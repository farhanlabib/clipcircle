# Local changes

Vendored from https://github.com/Shakibuzzaman3104/claude-jev-funnel (Apache-2.0,
commit b028492, plugin version 1.2.0). Only `skills/jev/` was copied, plus `LICENSE`.

Changed in `scripts/jev.py`:

- Added a `commandcode` provider (`https://api.commandcode.ai/provider/v1/systemone`,
  model `typesafe/jev`, key from `CMD_API_KEY`). Everything else is unchanged.
