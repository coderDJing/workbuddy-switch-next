# Daily task adapter

Pinned upstream code:

- WorkBuddy Daily, L0NE-6/WorkBuddy-Daily commit `db08e48d5bb04bae0079a23a827f1ef34ba3c65b`, MIT; see `upstream/workbuddy/LICENSE` and `NOTICE.md`.
- Qoder Daily, L0NE-6/Qoder-Daily commit `fa30b6a491906e7a2a730c0af2006bb2c6b6ce7d` (v1.0.19), MIT; see `upstream/qoder/LICENSE` and `NOTICE.md`.

The WorkBuddy snapshot has two integration patches: account loading runs only under `__main__`, and the local daemon lookup uses the session directory matching the active credential file, without an unused websocket import.

WorkBuddy v3.41 adds task-stage snapshots, delayed sign-in credit recognition, travel/blind-box ordering fixes, and narrower account-restriction detection. Upstream token-concurrency fixes are included in the snapshot; the adapter continues to leave token renewal to the host. The upstream import regression test explicitly loads accounts after checking that host imports never renew credentials.

`runner.py` adapts their per-account task functions to the host's stdin/stdout protocol. The host owns refresh-token rotation and the runner receives access tokens only. Windows desktop tasks use the existing daemon when its current login matches the target account, otherwise they use the upstream event fallback; neither path replaces the desktop login or deletes session files. Daily tasks may take many minutes and can use credits, energy, or create conversations, as in the upstream projects.
