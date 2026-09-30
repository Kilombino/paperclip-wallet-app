#!/usr/bin/env python3
"""Initialize the dedicated StartOS volume, then drop root before running the wallet."""
import os
from pathlib import Path

os.umask(0o077)
for directory in (Path('/data'), Path('/data/wallet')):
    if directory.is_symlink():
        raise SystemExit('Refusing a symbolic link for wallet storage')
    directory.mkdir(mode=0o700, exist_ok=True)
    os.chown(directory, 1000, 1000)
    os.chmod(directory, 0o700)
os.setgroups([])
os.setgid(1000)
os.setuid(1000)
os.environ['PAPERCLIP_XBT_MAINNET'] = '1'
os.execvp('python3', ['python3', '/usr/local/lib/paperclip/entrypoint.py'])
