"""Bind fixed experiment inputs to original native timing and usage evidence.

This is an evidence reader, not a scheduler or an approval authority. A local
controller must pin the selection before starting a fresh data root. Historical
runs without that observation remain exploratory. Provider bills and adversarial
same-user tampering are outside this local provenance claim.
"""
from datetime import datetime, timezone
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


bundle = module('record_materials', ROOT / 'scripts/efficiency-run-bundle.py')
usage = bundle.usage
require = bundle.require


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path, value):
    with Path(path).open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def reference(path):
    return dict(path=str(Path(path).resolve()), sha256=sha(path))


def selected(ref):
    require(set(ref) == {'path', 'sha256'}, 'Reference fields differ')
    require(re.fullmatch('[a-f0-9]{64}', ref['sha256']), 'Invalid selected digest')
    require(Path(ref['path']).is_absolute(), 'Selected path must be absolute')
    path = bundle.staging.no_links(Path(ref['path']))
    require(path.is_absolute() and path.is_file() and path.stat().st_nlink == 1,
            'Selected source must be an ordinary absolute file')
    require(sha(path) == ref['sha256'], 'Selected source bytes changed: ' + str(path))
    return path


def path_identity(path):
    text = str(Path(path).resolve())
    if text.startswith('\\\\?\\UNC\\'):
        text = '\\\\' + text[8:]
    elif text.startswith('\\\\?\\'):
        text = text[4:]
    return os.path.normcase(os.path.normpath(text))


def inspect_selection(selection):
    required = {'schema', 'classification', 'slotId', 'mode', 'dataRoot', 'baselineId',
                'request', 'composition', 'runtime', 'driver', 'authorization',
                'protocol', 'limits'}
    require(set(selection) == required and type(selection['schema']) is int and selection['schema'] == 1,
            'Experiment selection fields differ')
    require(selection['classification'] in ('exploratory', 'formal')
            and selection['mode'] in ('single', 'team'), 'Experiment classification differs')
    request = read(selected(selection['request']))
    composition = read(selected(selection['composition']))
    material = request if selection['mode'] == 'single' else request['material']
    manifest, _ = bundle.inspect(Path(material['bundle']), material['manifestSha256'])
    require(manifest['slot']['id'] == selection['slotId']
            and manifest['slot']['mode'] == selection['mode'], 'Selected slot differs')
    config = read(Path(material['bundle']) / 'configuration.json')
    require(config['executionKind'] == 'real-attempt', 'Synthetic configuration cannot be a real run')
    runtime = selection['runtime']
    require(set(runtime) == {'core', 'reader', 'sourcePins'}, 'Runtime fields differ')
    for ref in runtime.values():
        selected(ref)
    for name, digest in read(selected(runtime['sourcePins'])).items():
        require(sha(ROOT / name) == digest, 'Runtime source changed: ' + name)
    selected(selection['driver'])
    grant = read(selected(selection['authorization']))
    require(grant.get('noPerBatchReconfirmationRequired') is True
            and grant.get('unlimitedRequestBatchAuthorized') is False,
            'Bounded continuing authorization note required')
    limits = selection['limits']
    require(set(limits) == {'workerRequests', 'managementRequests', 'taskSeconds'}
            and all(type(v) is int and v >= 0 for v in limits.values())
            and 0 < limits['workerRequests'] <= 24 and limits['managementRequests'] <= 7
            and 0 < limits['taskSeconds'] <= 240, 'Unbounded or unsupported request limits')
    require((selection['mode'] == 'single' and limits['managementRequests'] == 0)
            or selection['mode'] == 'team', 'Baseline cannot have management requests')
    require(limits['taskSeconds'] == config['limits']['taskSeconds']
            and limits['managementRequests'] == config['limits']['managementRequests'],
            'Runtime/configuration limits differ')
    if selection['mode'] == 'single':
        require(isinstance(selection['baselineId'], str)
                and re.fullmatch('[a-zA-Z0-9_-]{1,64}', selection['baselineId']), 'Baseline identity required')
        require(limits['workerRequests'] == config['worker']['maxRounds'], 'Single round bound differs')
    else:
        require(selection['baselineId'] is None, 'Team cannot have a baseline identity')
        require(limits['workerRequests'] == min(config['limits']['workerRequests'],
                len(composition['plan']['workflow']['tasks']) * config['worker']['maxRounds']),
                'Team round/request bound differs')
    data = bundle.staging.no_links(Path(selection['dataRoot']))
    require(data.is_absolute() and data.is_relative_to(ROOT / 'reports'), 'Dedicated report data root required')
    if selection['mode'] == 'team':
        require(path_identity(material['dataRoot']) == path_identity(data), 'Team selected data root differs')
    for control in (Path(material['bundle']), Path(material['projectRoot']), selected(selection['request'])):
        require(not data.is_relative_to(control) and not control.is_relative_to(data),
                'Data/control/worker roots overlap')
    if selection['classification'] == 'formal':
        protocol = read(selected(selection['protocol']))
        require(type(protocol.get('schema')) is int and protocol.get('schema') == 1
                and protocol.get('kind') == 'fixed-72-slot-measurement'
                and protocol.get('efficiencyThreshold') is None
                and protocol.get('automaticRetry') is False, 'Formal protocol required')
        schedule = bundle.staging.load_suite()[2]
        require(protocol['slotOrder'] == [s['id'] for s in schedule['slots']]
                and len(set(protocol['slotOrder'])) == 72, 'Formal schedule changed or incomplete')
        require(selection['slotId'] in protocol['slotOrder']
                and type(protocol['maximumProviderRequests']) is int
                and 0 < protocol['maximumProviderRequests'] <= 1116,
                'Formal total request ceiling missing')
    else:
        require(selection['protocol'] is None, 'Exploratory run cannot borrow a formal protocol')
    return request, composition, manifest, config


def begin(selection_file, selection_sha, output):
    """Must execute before the first service start; never backfills an old run."""
    ref = dict(path=str(Path(selection_file).resolve()), sha256=selection_sha)
    selection = read(selected(ref))
    request, _, manifest, _ = inspect_selection(selection)
    data = Path(selection['dataRoot'])
    require(not data.exists(), 'Pre-run source root already exists; historical run cannot be enrolled')
    material = request if selection['mode'] == 'single' else request['material']
    project = Path(material['projectRoot'])
    for name, digest in manifest['slot']['seed'].items():
        require(sha(project / name) == digest, 'Pre-run seed changed')
    output = bundle.staging.no_links(Path(output))
    require(not output.is_relative_to(data) and not output.is_relative_to(project),
            'Pre-run seal must remain outside mutable roots')
    save(output, dict(schema=1, kind='pre-run-source-observation',
         observedAt=datetime.now(timezone.utc).isoformat(), selection=ref,
         module=reference(Path(__file__)), dataRoot=selection['dataRoot'],
         sourceRootAbsent=True, originalSourceInventory=[], modelCalls=0,
         slotId=selection['slotId'], classification=selection['classification']))
    return reference(output)


def validate_native(selection, request, composition, manifest, config, timing):
    """Validate an original native-reader response; does not itself run or authenticate a process."""
    require(timing.get('sourceValidated') is True, 'Original native source validation absent')
    terminal = timing.get('terminal')
    require(isinstance(terminal, dict), 'Original terminal incomplete; retain and reconcile')
    require(terminal['startDigest'] == timing['startDigest'], 'Timing start differs')
    mode = selection['mode']
    if mode == 'single':
        source = timing['sources']['baseline']
        require(source['materialProofAtPreparation'] == composition['proof']
                and source['slotId'] == selection['slotId']
                and source['configurationSha256'] == manifest['configurationSha256']
                and source['managementModelRequests'] == 0, 'Baseline material provenance differs')
        actual_request = dict(timing['start']['request'])
        expected_request = dict(request)
        actual_root = actual_request.pop('dataRoot')
        expected_request.pop('dataRoot')
        require(actual_request == expected_request, 'Baseline material request changed')
        expected_root = Path(selection['dataRoot']) / 'baseline-experiments' / selection['baselineId'] / 'data'
        require(path_identity(actual_root) == path_identity(expected_root)
                and path_identity(timing['start']['dataRoot']) == path_identity(expected_root),
                'Baseline runtime root differs')
        export = source['coreExport']
    else:
        source = timing['execution']
        require(source['materialContentBound'] is True and source['materialBinding'] == composition,
                'Team material/configuration binding differs')
        require(timing['start']['project'] == manifest['proposedProjectId']
                and timing['start']['setupDigest'] == composition['setup']['digest']
                and path_identity(timing['start']['dataRoot']) == path_identity(selection['dataRoot']),
                'Team original start differs')
        export = source['coreExport']
    require(export['projectId'] == manifest['proposedProjectId'], 'Export project differs')
    require(export['managementUsageIncluded'] is True, 'Complete management inventory required')
    normalized = usage.analyze(export)
    require(normalized['workerScopes'] <= selection['limits']['workerRequests']
            and normalized['managementScopes'] <= selection['limits']['managementRequests'],
            'Original request inventory exceeded frozen ceiling')
    require(mode != 'single' or normalized['managementScopes'] == 0, 'Baseline has management cost')
    for call in export['apiCalls']:
        fp = call['identity']['fingerprint']
        require(fp['target'] == config['worker']['target']
                and fp['requestedModel'] == config['worker']['model']
                and fp['maxOutputTokens'] == config['worker']['maxOutputTokens']
                and fp['timeoutSeconds'] == config['worker']['requestTimeoutSeconds'],
                'Worker model/endpoint/limits changed')
    models = {entry['role']: entry['model'] for entry in config['management']}
    for op in (export.get('management') or {}).get('operations', []):
        role = 'deputy' if op['purpose'] == 'assign' else 'chief'
        sources = {entry['role']: entry['source'].removesuffix('-api') for entry in config['management']}
        require(op['purpose'] in ('plan', 'assign', 'accept')
                and op['requestedModel'] == models.get(role)
                and op['source'] == sources.get(role), 'Management model/source/purpose changed')
    checks = export['verifications']
    accepted = terminal['accepted']
    require(type(accepted) is bool, 'Terminal quality classification missing')
    if accepted:
        require(checks and all(v['passed'] for v in checks)
                and all(t['state'] == 'accepted' and t['verified'] for t in export['taskStates'].values()),
                'Accepted terminal lacks independent verification')
    counters = normalized['counters']
    known = normalized['fullInputOutputKnownForExportedScopes']
    return dict(slotId=selection['slotId'], classification=selection['classification'],
        status='passed' if accepted else 'failed', independentCheckPassed=accepted,
        workerRequests=normalized['workerScopes'], managementRequests=normalized['managementScopes'],
        inputTokens=counters['inputTokens']['total'], outputTokens=counters['outputTokens']['total'],
        totalTokens=counters['inputTokens']['total'] + counters['outputTokens']['total'] if known else None,
        cachedInputTokens=counters['cachedInputTokens']['total'], cachedInputAlreadyIncluded=True,
        knownInputSubtotal=counters['inputTokens']['knownSubtotal'], knownOutputSubtotal=counters['outputTokens']['knownSubtotal'],
        wallMs=terminal['elapsedMs'], clockContinuity=terminal['clockContinuity'],
        humanActiveMs=terminal['humanActiveMs'], humanTimeBasis=terminal['humanTimeBasis'],
        managementRequestMs=None, actualCost=None, providerBillVerified=False,
        modelDifferences=normalized['modelDifferences'], unresolvedReviews=normalized['unresolvedReviewCount'],
        uncertainRemoteUsage=export['apiUncertainRemoteUsage'],
        nativeLowerLevelReadiness=terminal['realEfficiencyRecordReady'],
        runtimeEvidenceValidated=True, wholeInputOutputKnown=known)


def observe_native(reader_ref, argv, folder):
    """Only the two offline reader subcommands are allowed; never starts a host."""
    reader = selected(reader_ref)
    require(argv and argv[0] in ('timing-single', 'timing-team'), 'Live command refused by evidence reader')
    folder = Path(folder); folder.mkdir()
    observer = module('record_process_observer', ROOT / 'scripts/observe_owned_process.py')
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in
           ('KEY', 'TOKEN', 'SECRET', 'CREDENTIAL', 'AUTH', 'PASSWORD')) and not k.upper().startswith('CERULEAN_')}
    class System:
        def start(self, command, executable, run):
            return subprocess.Popen(command, executable=str(executable), cwd=ROOT, env=env,
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    state = {}
    child = observer.observe_launch(System(), [str(reader), *argv], reader, folder, 120,
                                   lambda process: None, state, save)
    save(folder / 'receipt.json', state)
    require(child.returncode == 0 and state['stdoutEOF'] and state['stderrEOF']
            and not state['initialTimeout'] and not state['observationErrors'],
            'Original reader failed or unconfirmed; do not retry')
    return read(folder / 'installer.stdout')


def source_inventory(data):
    """Retain every ordinary source file; never read a credential or hash a lock."""
    data = bundle.staging.no_links(Path(data))
    if os.name == 'nt' and not str(data).startswith('\\\\?\\'):
        data = Path('\\\\?\\' + str(data))
    inventory = {}
    for folder, directories, files in os.walk(data, followlinks=False):
        for name in directories + files:
            bundle.staging.no_links(Path(folder) / name)
        for name in files:
            path = Path(folder) / name
            require(path.suffix.lower() != '.dpapi', 'Credential remains in source inventory')
            require(path.stat().st_nlink == 1, 'Hardlinked source evidence refused')
            if name == 'writer.lock':
                continue
            inventory[path.relative_to(data).as_posix()] = sha(path)
    return inventory


def audit(selection_file, selection_sha, start_digest, output, begin_ref=None):
    """Cold source read, after normal service shutdown. Missing pre-seal stays exploratory."""
    ref = dict(path=str(Path(selection_file).resolve()), sha256=selection_sha)
    selection = read(selected(ref))
    request, composition, manifest, config = inspect_selection(selection)
    require(re.fullmatch('[a-f0-9]{64}', start_digest), 'Selected original start digest required')
    pre = None
    if begin_ref is not None:
        pre = read(selected(begin_ref))
        require(pre['kind'] == 'pre-run-source-observation' and pre['selection'] == ref
                and pre['module'] == reference(Path(__file__)) and pre['sourceRootAbsent'] is True
                and pre['originalSourceInventory'] == [] and pre['modelCalls'] == 0
                and pre['classification'] == selection['classification']
                and pre['slotId'] == selection['slotId'] and pre['dataRoot'] == selection['dataRoot'],
                'Pre-run source/selection binding differs')
    require(selection['classification'] != 'formal' or pre is not None,
            'Formal run lacks original pre-run seal; cannot backfill')
    output = bundle.staging.no_links(Path(output))
    data = Path(selection['dataRoot'])
    material = request if selection['mode'] == 'single' else request['material']
    mutable = (data, Path(material['projectRoot']), Path(material['worktree']['poolRoot']))
    require(all(not output.is_relative_to(p) and not p.is_relative_to(output) for p in mutable),
            'Audit output overlaps mutable roots')
    require(not (data / 'credentials/deepseek.dpapi').exists(), 'Temporary credential cleanup incomplete')
    before = source_inventory(data)
    output.mkdir()
    argv = (['timing-single', str(data / 'baseline-experiments' / selection['baselineId'] / 'timeline'), start_digest]
            if selection['mode'] == 'single' else ['timing-team', str(data), manifest['proposedProjectId'], start_digest])
    timing = observe_native(selection['runtime']['reader'], argv, output / 'native')
    result = validate_native(selection, request, composition, manifest, config, timing)
    require(before == source_inventory(data), 'Original evidence changed during native read')
    save(output / 'source-inventory.json', before)
    if pre is not None:
        require(datetime.fromisoformat(pre['observedAt']) <= datetime.fromisoformat(timing['start']['startedAt']),
                'Pre-run observation was recorded after original start')
    # Re-read all selected immutable inputs after the native observation.
    inspect_selection(selection)
    result.update(schema=1, kind='bound-original-experiment-record',
        at=datetime.now(timezone.utc).isoformat(), selection=ref, begin=begin_ref,
        sourceReader=selection['runtime']['reader'], sourceRead=reference(output / 'native/installer.stdout'),
        sourceInventory=reference(output / 'source-inventory.json'), originalSourceBytesUnchanged=True,
        preRunSourceObservationVerified=pre is not None,
        formalSlotRecorded=selection['classification'] == 'formal' and pre is not None,
        locallyRecordedProviderProvenance=True, cryptographicProviderAttestation=False,
        experimentalGainProven=False, formalAcceptance=False,
        limitations=['Local trusted controller provenance; not a provider signature or invoice.',
                    'Failure and unknown usage remain in records; no automatic retry.',
                    'Human active time uses explicit spans, not inferred attention.'])
    save(output / 'record.json', result)
    return result
