#!/usr/bin/env python3
"""Local authenticated daemon liveness, including first-run setup mode."""
from pathlib import Path
import sys
from urllib.request import Request, urlopen

try:
    token = Path('/data/wallet/auth_token').read_text()
    request = Request('http://127.0.0.1:3000/api/v1/wallet', headers={'Authorization': 'Bearer ' + token})
    with urlopen(request, timeout=5) as response:
        if response.status != 200:
            raise ValueError('Not ready')
except Exception:
    sys.exit('Wallet API is not ready')
