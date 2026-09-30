#!/usr/bin/env python3
"""Authenticated StartOS owner action; never expose this through the wallet HTTP API."""
import json
from pathlib import Path
import sys

path = Path('/data/wallet/auth_token')
if not path.is_file():
    sys.exit('Start the wallet service once before retrieving its token.')
print(json.dumps({'version': '0', 'message': 'Wallet access token (grants spending access)',
                  'value': path.read_text(), 'copyable': True, 'qr': False}))
