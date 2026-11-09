import importlib.util
import sys
import types
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("daily_adapter", __file__.replace("test_runner.py", "runner.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_upstream_messages_never_echo_access_token(self):
        self.assertEqual(runner.scrub({"message": "bad token-secret", "remaining": ["token-secret"]}, "token-secret"),
                         {"message": "bad [凭据]", "remaining": ["[凭据]"]})

    def test_cn_runs_full_workflow_without_refresh_token_or_local_session_deletion(self):
        calls = []
        module = types.ModuleType("workbuddy_daily")
        module._desktop_fingerprint_fallback = lambda *args: calls.append("desktop")
        module._market_expert_by_id = lambda *args: None
        module.prog = lambda *_: (None, 0, 0)
        def execute(index, account, desktop):
            self.assertEqual(account, {"access_token": "test-token", "note": ""})
            self.assertTrue(desktop)
            module.t_desktop_tasks(None, "u", "n", "test-token", None, True, True)
            self.assertEqual(module._web_conversation_prune(None, "test"), 0)
            module.t_workstation(None, "u", "n", lambda _: None, "test-token")
            return [], {"done": 2, "total": 3, "rest": ["待完成"]}
        module.run_account = execute
        with patch.dict(sys.modules, {"workbuddy_daily": module}), patch.object(runner, "is_current_desktop_account", return_value=False):
            result = runner.workbuddy_cn("test-token")
        self.assertEqual(calls, ["desktop"])
        self.assertFalse(result["ok"])
        self.assertEqual(result["remaining"], ["待完成"])

    def test_workstation_is_not_started_if_not_offered(self):
        module = types.ModuleType("workbuddy_daily")
        module.prog = lambda *_: (None, 0, 0)
        def run(*_):
            module.t_workstation(None, "u", "n", lambda _: None, "token")
            return [], {"done": 1, "total": 1, "rest": []}
        module.run_account = run
        with patch.dict(sys.modules, {"workbuddy_daily": module}):
            result = runner.workbuddy_cn("token")
        self.assertTrue(result["ok"])

    def test_real_desktop_tasks_run_only_for_matching_login_and_fall_back_per_task(self):
        module = types.SimpleNamespace()
        calls = []
        module.BASE = "https://example.invalid"
        module.SKILL_NAME = "fixture-skill"
        module.daemon_chat = lambda *args, **kwargs: calls.append("desktop") or "OK"
        module.ensure_local_skill = lambda: calls.append("install-skill")
        module.prog = lambda session, code: ("completed" if code == "RichMeow_Chat" else "accepted", 0, 1)
        module.claim = lambda session, code, log: calls.append("claim:" + code)
        module._desktop_fingerprint_fallback = lambda session, uid, nick, log, rich, skill: calls.append(("fallback", rich, skill))
        response = types.SimpleNamespace(json=lambda: {"data":{"sources":[{"id":"market"}]}})
        session = types.SimpleNamespace(get=lambda *a, **kw: response, post=lambda *a, **kw: None)
        with tempfile.TemporaryDirectory() as folder:
            module.INFO_PATH = str(Path(folder) / "auth.json")
            Path(module.INFO_PATH).write_text('{"auth":{"accessToken":"current"}}', encoding="utf-8")
            with patch.object(runner, "sys", types.SimpleNamespace(platform="win32")):
                runner.run_desktop_tasks(module, session, "u", "n", "current", lambda _: None, True, True)
                runner.run_desktop_tasks(module, session, "u", "n", "other", lambda _: None, True, False)
        self.assertEqual(calls, ["desktop", "install-skill", "desktop", "claim:RichMeow_Chat", ("fallback", False, True), ("fallback", True, False)])

    def test_global_preflight_blocks_restricted_account_before_writes(self):
        module = types.ModuleType("workbuddy_intl_daily")
        class Restricted(Exception): pass
        module.AccountRestricted = Restricted
        module.UnapprovedChannel = type("UnapprovedChannel", (Exception,), {})
        module.uid_of = lambda _: "uid"
        def preflight(*_): raise Restricted()
        module.preflight_restricted = preflight
        with patch.dict(sys.modules, {"workbuddy_intl_daily": module}):
            result = runner.workbuddy_global("token", "")
        self.assertFalse(result["ok"])
        self.assertIn("受限", result["message"])

    def test_global_runs_claim_report_keepalive_and_web_conversation(self):
        module = types.ModuleType("workbuddy_intl_daily")
        calls = []
        module.AccountRestricted = type("Restricted", (Exception,), {})
        module.UnapprovedChannel = type("UnapprovedChannel", (Exception,), {})
        module.preflight_restricted = lambda *_: (types.SimpleNamespace(status_code=200), {})
        module.detect_activity = lambda *_: (True, True, False, "可领取")
        def step(name):
            def run(*_):
                calls.append(name)
                return True, name
            return run
        module.claim_daily = step("claim")
        module.report_activity = step("report")
        module.keepalive = step("keepalive")
        module.web_conversation = step("web")
        module.day_completed_note = lambda message, *_: message
        with patch.dict(sys.modules, {"workbuddy_intl_daily": module}):
            result = runner.workbuddy_global("token", "uid")
        self.assertTrue(result["ok"])
        self.assertEqual(calls, ["claim", "report", "keepalive", "web"])

    def test_global_outside_window_skips_claim_but_runs_activity(self):
        module = types.ModuleType("workbuddy_intl_daily")
        calls = []
        module.AccountRestricted = type("Restricted", (Exception,), {})
        module.UnapprovedChannel = type("UnapprovedChannel", (Exception,), {})
        module.preflight_restricted = lambda *_: (types.SimpleNamespace(status_code=200), {})
        module.detect_activity = lambda *_: (True, True, False, "可领取")
        def step(name):
            def run(*_):
                calls.append(name)
                return True, name
            return run
        module.claim_daily = step("claim")
        module.report_activity = step("report")
        module.keepalive = step("keepalive")
        module.web_conversation = step("web")
        with patch.dict(sys.modules, {"workbuddy_intl_daily": module}):
            result = runner.workbuddy_global("token", "uid", True)
        self.assertTrue(result["ok"])
        self.assertEqual(calls, ["report", "keepalive", "web"])
        self.assertIn("不在签到时间段", result["message"])

    def test_cn_outside_window_skips_only_signin(self):
        module = types.ModuleType("workbuddy_daily")
        calls = []
        module.t_daily_activity = lambda *_: calls.append("activity")
        def run(*_):
            module.t_sign(None, "uid", "nick", lambda _: None, {})
            return [], {"done": 1, "total": 1, "rest": []}
        module.run_account = run
        with patch.dict(sys.modules, {"workbuddy_daily": module}):
            result = runner.workbuddy_cn("token", True)
        self.assertTrue(result["ok"])
        self.assertEqual(calls, ["activity"])

    def test_global_forbidden_preflight_never_starts_activity(self):
        module = types.ModuleType("workbuddy_intl_daily")
        module.AccountRestricted = type("Restricted", (Exception,), {})
        module.UnapprovedChannel = type("UnapprovedChannel", (Exception,), {})
        module.preflight_restricted = lambda *_: (types.SimpleNamespace(status_code=403), {})
        with patch.dict(sys.modules, {"workbuddy_intl_daily": module}):
            result = runner.workbuddy_global("token", "uid")
        self.assertFalse(result["ok"])
        self.assertIn("预检", result["message"])

    def test_cn_channel_rejection_is_not_auth_failure(self):
        module = types.ModuleType("workbuddy_daily")
        module.run_account = lambda *_: ([], {"channel_rejected": True, "rest": ["渠道拦截"]})
        with patch.dict(sys.modules, {"workbuddy_daily": module}):
            result = runner.workbuddy_cn("token")
        self.assertFalse(result["ok"])
        self.assertNotIn("authInvalid", result)
        self.assertIn("11-128", result["message"])

    def test_global_channel_rejection_stops_before_activity(self):
        module = types.ModuleType("workbuddy_intl_daily")
        module.AccountRestricted = type("Restricted", (Exception,), {})
        module.UnapprovedChannel = type("UnapprovedChannel", (Exception,), {})
        def rejected(*_):
            raise module.UnapprovedChannel("fixture")
        module.preflight_restricted = rejected
        with patch.dict(sys.modules, {"workbuddy_intl_daily": module}):
            result = runner.workbuddy_global("token", "uid")
        self.assertFalse(result["ok"])
        self.assertNotIn("authInvalid", result)
        self.assertIn("11-128", result["message"])

    def test_qoder_runs_claim_and_pro_without_refresh_token(self):
        module = types.ModuleType("qoder_daily")
        module.REGIONS = {"global": {"base": "https://example.invalid"}}
        def daily(account):
            self.assertEqual(account["refresh"], "")
            self.assertEqual(account["uid"], "user")
            account["failed"] = False
            return True, "每日已领"
        module.do_checkin = daily
        module.do_pro_upgrade = lambda account: (True, False, "Pro 已处理")
        with patch.dict(sys.modules, {"qoder_daily": module}):
            result = runner.qoder("token", "user", True, "/tmp/state")
        self.assertTrue(result["ok"])
        self.assertIn("每日已领", result["message"])
        self.assertIn("Pro 已处理", result["message"])

if __name__ == "__main__":
    unittest.main()
