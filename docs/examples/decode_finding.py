# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
# examples/decode_finding.py
"""
Decode a single Finding::Result JSONL envelope from stdin and print a
summary of its decoded raw arrays.

This demonstrates:
- How to read a Finding envelope as one line of NDJSON from stdin
- How to discriminate by envelope variant (the "kind" tag)
- How to decode a RawArray {dtype, shape, data} base64 payload via numpy
- How to re-compute a simple statistic (the lag-1 autocorrelation
  estimate) from the decoded `returns` series and compare to the
  envelope's headline `effect.value`

Usage:
  miner scan stats.autocorr.ljung_box@1 \\
    --instrument EURUSD:bid \\
    --timeframe 15m \\
    --window 2024-06-12:2024-06-13 \\
    --param lags=5 \\
  | python docs/examples/decode_finding.py

Requires: numpy >= 1.20
"""

import base64
import json
import sys

import numpy as np


def decode_raw_array(raw_array):
    """Decode one RawArray dict {dtype, shape, data} into a numpy ndarray."""
    dtype = np.dtype(raw_array["dtype"])
    shape = tuple(raw_array["shape"])
    payload = base64.b64decode(raw_array["data"])
    return np.frombuffer(payload, dtype=dtype).reshape(shape)


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        envelope = json.loads(line)
        # Every envelope is a tagged object with a "kind" field; only
        # "result" carries data_slice + effect + raw payloads.
        if envelope.get("kind") != "result":
            continue
        print(f"scan_id       = {envelope['scan_id@version']}")
        print(f"instruments   = {envelope['instruments']}")
        print(f"timeframe     = {envelope['timeframe']}")
        print(f"effect.metric = {envelope['effect']['metric']}")
        print(f"effect.value  = {envelope['effect']['value']}")

        raw = envelope.get("raw", {}).get("series", {})
        for key, raw_array in raw.items():
            arr = decode_raw_array(raw_array)
            preview = arr.flat[:5].tolist() if arr.size > 0 else []
            print(
                f"  raw[{key!r}]: dtype={arr.dtype}, shape={arr.shape}, "
                f"first 5 = {preview}"
            )

        # Independent re-check: lag-1 autocorr from the decoded `returns`
        # series. Should be in the same ballpark as the headline statistic
        # for an autocorrelation scan.
        if "returns" in raw:
            returns = decode_raw_array(raw["returns"])
            if returns.size > 2:
                lag1 = np.corrcoef(returns[:-1], returns[1:])[0, 1]
                print(f"  lag-1 autocorr (re-computed) = {lag1:.6f}")
        return  # only handle the first Result envelope


if __name__ == "__main__":
    main()
