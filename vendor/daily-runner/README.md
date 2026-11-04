# Daily task adapter

Pinned upstream code:

- WorkBuddy Daily, L0NE-6/WorkBuddy-Daily commit `aaf43f3ab1bb2915f80474b742c5e9611265502a`, MIT; see `upstream/workbuddy/LICENSE` and `NOTICE.md`.
- Qoder Daily, coderDJing/Qoder-Daily-private commit `ffeae0c8cc51dfc4a720d506e47947b39b04578a`, MIT; see `upstream/qoder/LICENSE`.

The WorkBuddy snapshot has two integration patches: account loading runs only under `__main__`, and the local daemon lookup uses the session directory matching the active credential file, without an unused websocket import.

`runner.py` adapts their per-account task functions to the host's stdin/stdout protocol. The host owns refresh-token rotation and the runner receives access tokens only. Windows desktop tasks use the existing daemon when its current login matches the target account, otherwise they use the upstream event fallback; neither path replaces the desktop login or deletes session files. Daily tasks may take many minutes and can use credits, energy, or create conversations, as in the upstream projects.
