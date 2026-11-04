import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import qoder_daily as daily


def response(payload, status=200):
    result = Mock(status_code=status)
    result.json.return_value = payload
    return result


def account(access='dt-original', refresh='drt-original'):
    return {'access': access, 'refresh': refresh, 'note': '主号', 'region': 'cn',
            'base': daily.REGIONS['cn']['base']}


class NativeTests(unittest.TestCase):
    def setUp(self):
        daily.INSPECT = daily.ONLY = daily.REFRESH_ONLY = daily.NO_NOTIFY = False
        daily.PRIVATE_VALUES.clear()

    def test_quota_403_does_not_refresh_credentials(self):
        for payload in ({"code": 112}, {"code": 110}, {"pricingUrl": "https://example.test"}):
            with self.subTest(payload=payload):
                reply = response(payload, 403)
                reply.text = json.dumps(payload)
                with patch.object(daily, 'request', return_value=reply), \
                     patch.object(daily, 'risky_sash_headers', return_value=({}, None)), \
                     patch.object(daily, 'refresh_account') as refresh:
                    result, _ = daily.fetch_campaigns(account())
                    self.assertIs(result, reply)
                    self.assertTrue(daily.quota_error(result))
                    refresh.assert_not_called()
        ordinary = response({}, 403)
        ordinary.text = ''
        self.assertEqual('', daily.quota_error(ordinary))

    def test_rotation_is_found_from_unchanged_secret_after_multiple_runs(self):
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(daily, 'STORE_FILE', str(Path(folder) / 'tokens.json')):
                first = account()
                daily.apply_store(first)
                first.update(access='dt-rotated', refresh='drt-rotated')
                daily.persist(first)
                second = account()
                self.assertTrue(daily.apply_store(second))
                self.assertEqual(second['refresh'], 'drt-rotated')
                second.update(access='dt-third', refresh='drt-third')
                daily.persist(second)
                third = account()
                self.assertTrue(daily.apply_store(third))
                self.assertEqual(third['refresh'], 'drt-third')

    def test_native_multiline_and_ampersand_accounts(self):
        accounts = daily.parse_accounts('主号|dt-first|drt-first\n备号|dt-second|drt-second&dt-third|drt-third')
        self.assertEqual(len(accounts), 3)
        self.assertEqual(accounts[1]['note'], '备号')
        self.assertEqual(accounts[2]['refresh'], 'drt-third')

    def test_window_retry_rechecks_then_verifies_grant_receipt(self):
        row = {'campaignId': 'act-1', 'actionType': 'CLAIM_BENEFIT',
               'claimStatus': 'CLAIMABLE', 'benefit': {'kind': 'CREDITS', 'amount': 100}}
        clock = Mock()
        clock.now.return_value.hour = 10
        grant = {'grantId': 'grant-1', 'benefit': {'kind': 'CREDITS', 'amount': 100}}
        with patch.object(daily, 'datetime', clock), patch.object(daily.time, 'sleep') as sleep, \
             patch.object(daily, 'fetch_campaigns', side_effect=[
                 (response({'campaigns': []}), None),
                 (response({'campaigns': [row]}), None)]), \
             patch.object(daily, 'claim_campaign', return_value=response({'status': 'CLAIMED'})) as claim, \
             patch.object(daily, 'campaign_reward', return_value=grant):
            ok, message = daily.do_checkin(account())
        self.assertTrue(ok)
        self.assertIn('100', message)
        sleep.assert_called_once_with(600)
        claim.assert_called_once()

    def test_inspect_never_claims(self):
        daily.INSPECT = True
        row = {'campaignId': 'act-1', 'actionType': 'CLAIM_BENEFIT',
               'claimStatus': 'CLAIMABLE', 'benefit': {'kind': 'CREDITS', 'amount': 100}}
        with patch.object(daily, 'fetch_campaigns', return_value=(response({'campaigns': [row]}), None)), \
             patch.object(daily, 'claim_campaign') as claim:
            success, message = daily.do_checkin(account())
        self.assertFalse(success)
        self.assertIn('仅查询', message)
        claim.assert_not_called()

    def test_coupons_are_not_misreported_as_daily_credits(self):
        row = {'campaignId': 'coupon-1', 'actionType': 'CLAIM_BENEFIT',
               'claimStatus': 'CLAIMABLE', 'benefit': {'kind': 'REDEMPTION_CODE'}}
        with patch.object(daily, 'fetch_campaigns', return_value=(response({'campaigns': [row]}), None)), \
             patch.object(daily, 'claim_campaign') as claim:
            daily.do_checkin(account())
        claim.assert_not_called()

    def test_claim_requires_grant_and_does_not_use_list_amount(self):
        row = {'campaignId': 'act-1', 'actionType': 'CLAIM_BENEFIT',
               'claimStatus': 'CLAIMABLE', 'benefit': {'kind': 'CREDITS', 'amount': 100}}
        item = account()
        with patch.object(daily, 'fetch_campaigns', return_value=(response({'campaigns': [row]}), None)), \
             patch.object(daily, 'claim_campaign', return_value=response({'status': 'CLAIMED'})), \
             patch.object(daily, 'campaign_reward', return_value={}):
            success, message = daily.do_checkin(item)
        self.assertFalse(success)
        self.assertTrue(item['failed'])
        self.assertIn('待确认', message)

    def test_claim_receipt_confirms_real_amount(self):
        row = {'campaignId': 'act-1', 'actionType': 'CLAIM_BENEFIT',
               'claimStatus': 'CLAIMABLE', 'benefit': {'kind': 'CREDITS', 'amount': 100}}
        item = account()
        with patch.object(daily, 'fetch_campaigns', return_value=(response({'campaigns': [row]}), None)), \
             patch.object(daily, 'claim_campaign', return_value=response({'status': 'CLAIMED'})), \
             patch.object(daily, 'campaign_reward', return_value={'grantId': 'g1', 'benefit': {'amount': 50}}):
            success, message = daily.do_checkin(item)
        self.assertTrue(success)
        self.assertEqual(item['receipt']['new_credit'], 50)

    def test_previous_grant_is_not_new(self):
        row = {'campaignId': 'act-1', 'actionType': 'CLAIM_BENEFIT',
               'claimStatus': 'CLAIMED', 'benefit': {'kind': 'CREDITS'}}
        item = account()
        with patch.object(daily, 'fetch_campaigns', return_value=(response({'campaigns': [row]}), None)), \
             patch.object(daily, 'campaign_reward', return_value={'grantId': 'g1', 'benefit': {'amount': 100}}):
            daily.do_checkin(item)
        self.assertEqual(item['receipt']['new_credit'], 0)
        self.assertEqual(item['receipt']['previous_credit'], 100)

    def test_pro_success_flag_does_not_invent_amount(self):
        with patch.object(daily, 'risky_sash_headers', return_value=({}, None)), \
             patch.object(daily, 'request', side_effect=[response({'eligible': True}), response({'success': True})]):
            item = account()
            self.assertEqual(daily.do_pro_upgrade(item)[:2], (False, False))
            self.assertTrue(item['failed'])

    def test_masking_covers_rotated_credentials(self):
        daily.protect('dt-rotated-private')
        self.assertNotIn('dt-rotated-private', daily.safe('failure: dt-rotated-private'))
        self.assertNotIn('drt-other-value', daily.safe('drt-other-value'))

    def test_missing_credentials_is_failure(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(sys, 'argv', ['qoder_daily.py']):
            self.assertEqual(daily.main(), 2)

    def test_failed_account_does_not_stop_next_and_run_fails(self):
        calls = []
        def balances(item):
            calls.append(item['access'])
            if item['access'] == 'dt-bad':
                raise RuntimeError('private-error-response')
            return 100
        with patch.dict(os.environ, {'QODER_TOKENS': 'dt-bad|drt-bad\ndt-good|drt-good'}, clear=True), \
             patch.object(sys, 'argv', ['qoder_daily.py', '--no-notify']), \
             patch.object(daily, 'apply_store'), patch.object(daily, 'fetch_balance', side_effect=balances), \
             patch.object(daily, 'do_checkin', return_value=(False, '今日已领取')), \
             patch.object(daily, 'do_pro_upgrade', return_value=(True, False, '不可领取')), \
             patch.object(daily.time, 'sleep'):
            self.assertEqual(daily.main(), 1)
        self.assertIn('dt-good', calls)


if __name__ == '__main__':
    unittest.main()
