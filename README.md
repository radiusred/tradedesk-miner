# tradedesk-miner

**Temporarily unpublished — the code returns shortly.**

`tradedesk-miner` is RadiusRed's open-source (Apache-2.0) data-mining and
backtesting engine for historical OHLCV data: the scan catalogue, the
aggregate-bar cache, the `miner` CLI/MCP/HTTP surfaces and the
`miner-backtest` crate.

During recent work the repository came to carry material that belongs to our
private research alongside the engine. We have taken the original repository
private while we separate the two. The engine comes back here under the same
licence, most likely as crates depending on a shared `tradedesk` data layer;
strategy implementations and research artefacts do not, and will live
privately.

Releases v1.0.2–v1.3.0 that were published from the original repository
remain valid under Apache-2.0 for anyone who obtained them.

Questions: open an issue here.
