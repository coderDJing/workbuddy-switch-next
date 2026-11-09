#!/usr/bin/env python3
"""One-account adapter for the pinned WorkBuddy Daily and Qoder Daily scripts.

The host sends one JSON request on stdin; tokens never enter argv, environment,
logs, or the response. Token renewal belongs exclusively to the host.
"""
import contextlib
import io
import json
import os
import sys


def setup(request):
    state = request.get("stateDir")
    if not isinstance(state, str) or not os.path.isabs(state):
        raise ValueError("invalid state directory")
    os.makedirs(state, mode=0o700, exist_ok=True)
    # Do not let shell/Actions configuration silently alter a desktop refresh.
    for key in list(os.environ):
        if key.startswith("WORKBUDDY_") or (key.startswith("QODER_") and key not in {
            "QODER_UMID_PATH", "QODER_UMID_NO_INSTALL", "QODER_MACHINE_TOKEN",
            "QODER_MACHINE_CODE", "QODER_MACHINE_TYPE",
        }):
            os.environ.pop(key, None)
    os.environ["WORKBUDDY_STATE_DIR"] = state
    os.environ["QODER_WINDOW_RETRY"] = "0"
    # Upstream scripts disable TLS validation. Retain normal certificate
    # verification, including for the registry metadata used to install UMID.
    import requests
    original_request = requests.sessions.Session.request
    def verified_request(session, method, url, **kwargs):
        kwargs["verify"] = True
        return original_request(session, method, url, **kwargs)
    requests.sessions.Session.request = verified_request


def is_current_desktop_account(daily, token):
    if sys.platform != "win32":
        return False
    directory = os.path.dirname(daily.INFO_PATH)
    paths = [daily.INFO_PATH, os.path.join(directory, "workbuddy-desktop.info"),
             os.path.join(directory, "workbuddy-desktop-ai.info")]
    for path in dict.fromkeys(paths):
        try:
            with open(path, encoding="utf-8") as handle:
                info = json.load(handle)
            if token and info.get("auth", {}).get("accessToken") == token:
                daily.INFO_PATH = path
                return True
        except (OSError, ValueError, AttributeError):
            continue
    return False


def run_desktop_tasks(daily, session, uid, nick, token, log, rich, skill):
    if not is_current_desktop_account(daily, token):
        return daily._desktop_fingerprint_fallback(session, uid, nick, log, rich, skill)
    if rich:
        try:
            daily.daemon_chat("你好，请用一句话介绍你自己")
        except Exception:
            log("桌面对话未完成，尝试网络回退")
    if skill:
        try:
            sources = session.get(daily.BASE + "/console/as/marketplace/sources", timeout=20).json()
            rows = (sources.get("data") or {}).get("sources") or []
            if rows and rows[0].get("id"):
                session.post(daily.BASE + "/console/as/user/plugins/install", json={"plugin_name": daily.SKILL_NAME, "marketplace_id": rows[0]["id"], "version": "latest"}, timeout=30)
            daily.ensure_local_skill()
            blocks = [{"type":"resource_link", "uri":"skill://" + daily.SKILL_NAME,
                       "title":daily.SKILL_NAME, "name":daily.SKILL_NAME,
                       "_meta":{"mentionType":"skill", "skillName":daily.SKILL_NAME}},
                      {"type":"text", "text":"你必须通过技能系统正式加载（load）该技能，加载成功后回答：技能已加载"}]
            daily.daemon_chat("", blocks=blocks)
        except Exception:
            log("桌面技能任务未完成，尝试网络回退")
    def pending(code):
        try:
            status = daily.prog(session, code)[0]
            if status == "completed":
                daily.claim(session, code, log)
            return status not in ("completed", "claimed")
        except Exception:
            return True
    need_rich = rich and pending("RichMeow_Chat")
    need_skill = skill and pending("skill_1")
    if need_rich or need_skill:
        daily._desktop_fingerprint_fallback(session, uid, nick, log, need_rich, need_skill)


def workbuddy_cn(token, skip_checkin=False):
    import workbuddy_daily as daily

    if skip_checkin:
        # Daily activity is normally nested inside upstream's sign-in handler;
        # keep it running even when the sign-in window is closed.
        daily.t_sign = lambda s, uid, nick, log, status=None: daily.t_daily_activity(s, uid, nick, log)
    # Upstream's Windows path force-closes WorkBuddy, deletes its session files,
    # and overwrites its active login. Use the upstream network fallback instead.
    daily.t_desktop_tasks = lambda s, uid, nick, tok, log, rich, skill: run_desktop_tasks(daily, s, uid, nick, tok, log, rich, skill)
    # Upstream's capacity recovery can stop unrelated active conversations.
    # Do not touch the user's existing sessions; a full queue is reported in
    # the final task status and can be retried after the user frees space.
    daily._web_conversation_prune = lambda *args, **kwargs: 0
    def workstation(s, uid, nick, log, current_token):
        status = daily.prog(s, "workstation_expert")[0]
        if status is None or status in ("completed", "claimed"):
            return
        if is_current_desktop_account(daily, current_token):
            try:
                blocks = [{"type":"resource_link", "uri":"expert://WorkspaceBuilder", "title":"工作台搭建师", "name":"工作台搭建师", "_meta":{"mentionType":"expert", "expertId":"WorkspaceBuilder"}}, {"type":"text", "text":"你好，请介绍你能帮我搭建什么工作台，回答OK即可"}]
                daily.daemon_chat("", blocks=blocks)
                if daily.prog(s, "workstation_expert")[0] in ("completed", "claimed"):
                    return
            except Exception:
                log("工作台搭建师桌面会话未完成，尝试网络回退")
        expert = daily._market_expert_by_id(s, "工作台搭建师", "")
        if expert:
            daily._run_local_expert_task(s, uid, nick, log, "workstation_expert", expert, "workstation")
        else:
            log("工作台搭建师：未找到专家，等待下次重试")
    daily.t_workstation = workstation
    lines, summary = daily.run_account(1, {"access_token": token, "note": ""}, True)
    if summary.get("channel_rejected"):
        return {"ok": False, "message": "请求渠道被拒绝（11-128），本轮已停止"}
    if summary.get("restricted"):
        return {"ok": False, "message": "账号受限，本轮已停止"}
    if summary.get("enterprise"):
        return {"ok": False, "message": "企业账号未开放个人成长任务"}
    if summary.get("rest") == ["凭据失效"]:
        return {"ok": False, "authInvalid": True, "message": "登录状态失效"}
    done, total = summary.get("done", 0), summary.get("total", 0)
    rest = [str(item)[:80] for item in summary.get("rest", [])[:30]]
    if not total:
        return {"ok": False, "message": "未能确认成长任务状态", "remaining": rest}
    warnings = [str(line)[:240] for line in lines if "⚠" in str(line) or "❌" in str(line)][:10]
    return {"ok": not rest and not warnings, "message": "成长任务完成 %s/%s%s" % (done, total, "；部分任务尚未完成" if rest else ""), "done": done, "total": total, "remaining": rest, "warnings": warnings}


def workbuddy_global(token, uid, skip_checkin=False):
    import workbuddy_intl_daily as daily

    uid = uid or daily.uid_of(token)
    try:
        response, _ = daily.preflight_restricted(token, uid)
    except daily.UnapprovedChannel:
        return {"ok": False, "message": "请求渠道被拒绝（11-128），本轮已停止"}
    except daily.AccountRestricted:
        return {"ok": False, "message": "账号受限，本轮已停止"}
    if response.status_code == 401:
        return {"ok": False, "authInvalid": True, "message": "登录状态失效"}
    if response.status_code == 403:
        return {"ok": False, "message": "账号状态预检被拒绝，本轮已停止"}
    available, active, checked, note = daily.detect_activity(token, uid)
    claim_ok = checked
    if skip_checkin:
        note = "不在签到时间段，已跳过每日领取"
    elif available and active and not checked:
        claim_ok, note = daily.claim_daily(token, uid)
    report_ok, _ = daily.report_activity(token, uid)
    keep_ok, _ = daily.keepalive(token, uid)
    web_ok, _ = daily.web_conversation(token, uid)
    message = note if skip_checkin else daily.day_completed_note(note, available, active, checked, claim_ok, report_ok, web_ok)
    return {"ok": bool((skip_checkin or claim_ok or not (available and active)) and report_ok and keep_ok and web_ok),
            "message": "%s；活跃上报%s，保活对话%s，网页会话%s" % (message, "成功" if report_ok else "失败", "成功" if keep_ok else "失败", "成功" if web_ok else "失败")}


def qoder(token, uid, global_region, state):
    import qoder_daily as daily
    daily.UMID_DIR = os.path.join(state, "qoder_umid")
    daily.MACHINE_ID_FILE = os.path.join(state, "qoder_machine_id")
    region = "global" if global_region else "cn"
    account = {"access": token, "refresh": "", "uid": uid, "region": region,
               "base": daily.REGIONS[region]["base"]}
    _, daily_note = daily.do_checkin(account)
    daily_ok = not account.get("failed", False)
    pro_ok, _, pro_note = daily.do_pro_upgrade(account)
    return {"ok": daily_ok and pro_ok, "message": "%s；%s" % (daily_note, pro_note),
            "authInvalid": "鉴权失败" in daily_note or "HTTP 401" in daily_note}


def dispatch(request):
    setup(request)
    if request.get("mode") == "self-check":
        import workbuddy_daily, workbuddy_intl_daily, qoder_daily
        return {"ok": True, "message": "每日任务组件就绪"}
    token = request.get("accessToken")
    if not isinstance(token, str) or not token:
        return {"ok": False, "message": "账号缺少登录凭据"}
    mode = request.get("mode")
    uid = request.get("uid") or ""
    if mode == "workbuddy-cn":
        return workbuddy_cn(token, bool(request.get("skipCheckin")))
    if mode == "workbuddy-global":
        return workbuddy_global(token, uid, bool(request.get("skipCheckin")))
    if mode == "qoder-cn" or mode == "qoder-global":
        return qoder(token, uid, mode == "qoder-global", request["stateDir"])
    return {"ok": False, "message": "未知任务类型"}


def scrub(value, token):
    """Never reflect a credential from an upstream message back to the host."""
    if isinstance(value, str):
        return value.replace(token, "[凭据]")[:1024] if token else value[:1024]
    if isinstance(value, list):
        return [scrub(item, token) for item in value[:100]]
    if isinstance(value, dict):
        return {key: scrub(item, token) for key, item in value.items()}
    return value


def main():
    try:
        request = json.loads(sys.stdin.buffer.readline(131073).decode("utf-8"))
        if not isinstance(request, dict):
            raise ValueError("invalid request")
        # Upstream emits verbose diagnostics; never send them through IPC as
        # they may contain server-provided account data or credential fragments.
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            result = dispatch(request)
            result = scrub(result, request.get("accessToken") if isinstance(request.get("accessToken"), str) else "")
    except Exception:
        result = {"ok": False, "message": "任务执行异常，请检查网络和登录状态"}
    print(json.dumps(result, ensure_ascii=True), flush=True)


if __name__ == "__main__":
    main()
