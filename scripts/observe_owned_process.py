"""Observe one owned setup child without abandoning it on evidence failures.

Both log files and readers exist before start(). A returned child stays owned
until its original wait reports a terminal exit and both readers finish their
pipe/log cleanup. This observer does not authorize retry, kill, or acceptance.
"""
from datetime import datetime, timezone
import json
import os
import subprocess
import threading
import time


def observe_launch(system, command, executable, run, timeout, bind, result, save_new):
    ready = threading.Event()
    errors, channels, threads = [], [], []
    error_lock = threading.Lock()
    child = None
    attempted = False
    timed_out = False
    exited = False
    wait_unknown_written = False

    def error(stage, exc):
        row = {'stage': stage, 'type': type(exc).__name__}
        with error_lock:
            if row not in errors and len(errors) < 64:
                errors.append(row)

    def preserve(stage, action):
        try:
            action()
            return True
        except BaseException as exc:
            error(stage, exc)
            return False

    def pause(stage):
        # A second interruption during backoff must not escape observation.
        preserve(stage, lambda: time.sleep(0.05))

    def close_stream(stage, stream):
        if stream is not None and not stream.closed:
            preserve(stage, stream.close)

    def read_pipe(channel):
        writable = True
        try:
            channel['entered'].set()
            ready.wait()
            pipe = channel['pipe']
            if pipe is None:
                return
            while True:
                chunk = pipe.read(65536)
                if not isinstance(chunk, bytes):
                    raise OSError('Observation pipe did not return bytes')
                if chunk == b'':
                    channel['eof'] = True
                    break
                channel['bytes'] += len(chunk)
                if writable:
                    try:
                        count = channel['file'].write(chunk)
                        if count != len(chunk):
                            raise OSError('Short observation log write')
                    except BaseException as exc:
                        error(channel['name'] + '-write', exc)
                        writable = False
                        # A failed disk write must not fill the child's PIPE.
            if writable:
                channel['file'].flush()
                os.fsync(channel['file'].fileno())
        except BaseException as exc:
            error(channel['name'] + '-capture', exc)
        finally:
            # done is our completion evidence. Thread.join/is_alive alone is
            # unsafe after an interrupted CPython join can mark _tstate stopped.
            try:
                close_stream(channel['name'] + '-pipe-close', channel['pipe'])
                close_stream(channel['name'] + '-log-close', channel['file'])
            except BaseException as exc:
                error(channel['name'] + '-cleanup', exc)
            finally:
                channel['done'].set()

    def publish_pipes():
        if child is not None:
            for channel, pipe in zip(channels, (child.stdout, child.stderr)):
                channel['pipe'] = pipe
        ready.set()

    def complete_owned_observation():
        nonlocal exited, timed_out, wait_unknown_written
        remaining_timeout = timeout
        published = False
        # Cleanup operations are protected as well as child.wait: repeated
        # interruptions of ready/wait/backoff cannot escape the owner.
        while not published:
            try:
                publish_pipes()
                published = True
            except BaseException as exc:
                error('publish-owned-pipes', exc)
                pause('publish-backoff')
        while child is not None and not exited:
            try:
                code = child.wait(timeout=remaining_timeout)
                if type(code) is not int or type(child.returncode) is not int or code != child.returncode:
                    raise OSError('Original wait did not provide a terminal exit code')
                exited = True
            except subprocess.TimeoutExpired:
                timed_out = True
                remaining_timeout = None
                preserve('timeout-observation.json', lambda: save_new(run / 'timeout-observation.json', {
                    'at': datetime.now(timezone.utc).isoformat(), 'pid': child.pid,
                    'status': 'observation-only-waiting-on-original-handle',
                    'retryAllowed': False, 'terminationAttempted': False}))
                preserve('timeout-notification', lambda: print(json.dumps({
                    'status': 'still-observing-original-process', 'pid': child.pid}), flush=True))
            except BaseException as exc:
                remaining_timeout = None
                error('original-wait', exc)
                if not wait_unknown_written:
                    wait_unknown_written = preserve('wait-unknown.json', lambda: save_new(run / 'wait-unknown.json', {
                        'status': 'original-wait-unresolved-locks-retained', 'pid': child.pid,
                        'retryAllowed': False, 'terminationAttempted': False}))
                pause('wait-backoff')
        for thread, channel in threads:
            # A failed Thread.start before entering its target cannot set done.
            # No Popen is attempted unless both readers acknowledge entry.
            started = channel['start_returned'] or channel['entered'].is_set() or thread.ident is not None
            if not started:
                continue
            while True:
                try:
                    if channel['done'].wait(0.2):
                        break
                except BaseException as exc:
                    error(channel['name'] + '-done-wait', exc)
                    pause('capture-done-backoff')
            while True:
                try:
                    thread.join()
                    break
                except BaseException as exc:
                    error(channel['name'] + '-join', exc)
                    pause('capture-join-backoff')
        # Unstarted readers still own logs. Retry a failed reader close once,
        # after its done event; the original error still prevents success.
        for channel in channels:
            close_stream(channel['name'] + '-pipe-close-final', channel['pipe'])
            close_stream(channel['name'] + '-log-close-final', channel['file'])

    try:
        for name in ('installer.stdout', 'installer.stderr'):
            channels.append({'name': name, 'file': (run / name).open('xb', buffering=0),
                             'pipe': None, 'eof': False, 'bytes': 0, 'entered': threading.Event(),
                             'done': threading.Event(), 'start_returned': False})
        for channel in channels:
            thread = threading.Thread(target=read_pipe, args=(channel,), daemon=False)
            threads.append((thread, channel))
            thread.start()
            channel['start_returned'] = True
        for _, channel in threads:
            channel['entered'].wait()
            if channel['done'].is_set():
                raise OSError('Observation reader failed before launch')
        attempted = True
        result.update(status='startup-unknown', launchAttempts=1, installerLaunches=None)
        child = system.start(command, executable, run)
        result.update(status='observing-original-process', installerLaunches=1)
        publish_pipes()
        preserve('bind-original-process', lambda: bind(child))
        preserve('original-process.json', lambda: save_new(run / 'original-process.json', {
            'at': datetime.now(timezone.utc).isoformat(), 'pid': child.pid,
            'handleValueForDiagnosticsOnly': int(child._handle), 'handleNotTransferable': True,
            'waitUsesOriginalPopenHandle': True}))
    except BaseException as exc:
        error('start-or-observation', exc)
    finally:
        # Protect re-entry into cleanup too. Completed events and the original
        # child are reused, never reset or replaced by a PID lookup.
        while True:
            try:
                complete_owned_observation()
                break
            except BaseException as exc:
                error('owned-observation-cleanup', exc)
                pause('owned-observation-backoff')

    result.update(observationErrors=errors, originalProcessObserved=exited,
                  installerExitCode=child.returncode if child is not None and exited else None,
                  stdoutEOF=len(channels) == 2 and channels[0]['eof'],
                  stderrEOF=len(channels) == 2 and channels[1]['eof'], initialTimeout=timed_out,
                  capturedBytes={c['name']: c['bytes'] for c in channels})
    if not attempted:
        result.update(status='prelaunch-preparation-failed', launchAttempts=0, installerLaunches=0)
    if child is not None and exited:
        preserve('finished.json', lambda: save_new(run / 'finished.json', {
            'at': datetime.now(timezone.utc).isoformat(), 'pid': child.pid, 'exit': child.returncode,
            'waitedOriginalHandle': True, 'stdoutEOF': result['stdoutEOF'],
            'stderrEOF': result['stderrEOF'], 'initialTimeout': timed_out,
            'observationErrors': list(errors)}))
    if errors or not exited or not result['stdoutEOF'] or not result['stderrEOF']:
        if child is not None:
            result['status'] = 'observation-evidence-failed'
        elif attempted:
            result['status'] = 'startup-unknown'
        raise RuntimeError('Setup observation failed; original operation and evidence are retained')
    return child
