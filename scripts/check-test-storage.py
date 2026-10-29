"""Read-only D:\test capacity preflight. No deletion and no symlink traversal."""
import argparse
from datetime import datetime, timezone, timedelta
import json
import os
from pathlib import Path
import shutil
import sys

def scan(root):
    root = Path(root).resolve()
    native = str(root)
    if os.name == 'nt' and not native.startswith('\\\\?\\'):
        native = '\\\\?\\' + native
    rows, errors, reparses = {}, [], []
    for first in os.scandir(native):
        total = count = 0
        stack = [first]
        while stack:
            entry = stack.pop()
            try:
                info = entry.stat(follow_symlinks=False)
                if getattr(info, 'st_file_attributes', 0) & 0x400 or entry.is_symlink():
                    reparses.append(entry.path)
                elif entry.is_dir(follow_symlinks=False):
                    with os.scandir(entry.path) as children:
                        stack.extend(children)
                else:
                    total += info.st_size
                    count += 1
            except OSError as error:
                errors.append({'path': entry.path, 'error': str(error)})
        rows[first.name] = {'bytes': total, 'files': count}
    return rows, errors, reparses

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--reserve-bytes', required=True, type=int)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--external-file', action='append', default=[], type=Path,
                        help='Explicit external test file, e.g. the user-provided ISO; counted once')
    args = parser.parse_args()
    if args.reserve_bytes < 0:
        parser.error('reserve must be nonnegative')
    if args.output.exists():
        parser.error('preserve old storage evidence; choose a new report path')
    root = Path('D:/test')
    rows, errors, reparses = scan(root)
    workspace_total = sum(row['bytes'] for row in rows.values())
    external = {}
    for supplied in args.external_file:
        file = supplied.resolve(strict=True)
        if file.is_relative_to(root.resolve()):
            continue
        if not file.is_file():
            parser.error('external allocation must name a file')
        external[str(file)] = file.stat().st_size
    total = workspace_total + sum(external.values())
    free = shutil.disk_usage(root).free
    limit = 120_000_000_000  # User raised the total test allowance on 2026-10-25.
    result = dict(at=datetime.now(timezone(timedelta(hours=8))).isoformat(),
        scope=str(root), limitBytes=limit, totalLogicalBytes=total,
        workspaceLogicalBytes=workspace_total, externalFiles=external,
        reserveBytes=args.reserve_bytes, projectedBytes=total + args.reserve_bytes,
        remainingBudgetBytes=limit-total, diskFreeBytes=free,
        diskSafetyReserveBytes=10_000_000_000, errors=errors,
        reparsePointsNotFollowed=reparses, topLevel=rows,
        passed=not errors and total+args.reserve_bytes <= limit and free >= args.reserve_bytes+10_000_000_000,
        limitation='Point-in-time preflight, not OS disk quota. Counts hardlinks per path conservatively. Reparse targets and other D-drive workspaces are outside this tally; include their test allocations separately before use.')
    with args.output.open('x', encoding='utf-8') as out:
        json.dump(result, out, ensure_ascii=False, indent=2)
        out.write('\n')
    print(json.dumps({k: result[k] for k in ('passed','totalLogicalBytes','remainingBudgetBytes','reserveBytes','diskFreeBytes') }))
    return 0 if result['passed'] else 2

if __name__ == '__main__':
    sys.exit(main())
