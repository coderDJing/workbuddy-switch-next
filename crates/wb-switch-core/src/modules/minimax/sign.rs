//! MiniMax `matrix` 接口的请求签名。
//!
//! 业务接口（`/matrix/api/v1/*`、`/v1/api/user/info`）不认裸 Bearer：
//! 官方客户端 `MemoryQuotaTransport.matrix` 还会带一组固定查询参数和两个签名头
//! （`yy` / `x-timestamp` + `x-signature`），缺了就统一返回 `statusInfo.code = 2`。
//! 参数与算法逐字对齐客户端（2026-11 版桌面端 `transport.js`）：
//!
//! ```text
//! yy          = md5( encodeURIComponent(pathname + search) + "_" + (body || "{}") + md5(ms) + "ooui" )
//! x-timestamp = floor(ms / 1000)
//! x-signature = md5( f"{x-timestamp}I*7Cf%WZ#S&%1RlZJ&C2{body}" )
//! ```
//!
//! `body` 在无请求体时按空串参与签名（`yy` 里则按 `"{}"`）。

/// RFC 1321 的 K 表（`floor(2^32 * |sin(i+1)|)`）。
const K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// 与 JS `crypto.createHash('md5')` 等价的十六进制摘要。
pub fn md5_hex(input: &[u8]) -> String {
    let (mut a0, mut b0, mut c0, mut d0) = (
        0x6745_2301u32,
        0xefcd_ab89u32,
        0x98ba_dcfeu32,
        0x1032_5476u32,
    );
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in message.chunks(64) {
        let mut words = [0u32; 16];
        for (index, word) in chunk.chunks(4).enumerate() {
            words[index] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for index in 0..64 {
            let (mixed, word) = match index {
                0..=15 => ((b & c) | (!b & d), index),
                16..=31 => ((d & b) | (!d & c), (5 * index + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * index + 5) % 16),
                _ => (c ^ (b | !d), (7 * index) % 16),
            };
            let sum = a
                .wrapping_add(mixed)
                .wrapping_add(K[index])
                .wrapping_add(words[word]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(sum.rotate_left(S[index]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// JS `encodeURIComponent`：保留 `A-Za-z0-9` 与 `-_.!~*'()`，其余按 UTF-8 百分号编码。
pub fn uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 三个签名头的值：`yy`、`x-timestamp`、`x-signature`。
pub fn headers(path_and_query: &str, body: &str, now_ms: i64) -> (String, String, String) {
    let yy = md5_hex(
        format!(
            "{}_{}{}ooui",
            uri_component(path_and_query),
            if body.is_empty() { "{}" } else { body },
            md5_hex(now_ms.to_string().as_bytes())
        )
        .as_bytes(),
    );
    let second = now_ms / 1000;
    let signature = md5_hex(format!("{second}I*7Cf%WZ#S&%1RlZJ&C2{body}").as_bytes());
    (yy, second.to_string(), signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_rfc_1321_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        // 跨块边界（>56 字节）也要正确填充。
        assert_eq!(
            md5_hex(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn uri_component_matches_javascript() {
        assert_eq!(
            uri_component("/v1/api/user/info"),
            "%2Fv1%2Fapi%2Fuser%2Finfo"
        );
        assert_eq!(uri_component("a=1&b=2"), "a%3D1%26b%3D2");
        assert_eq!(uri_component("-_.!~*'()"), "-_.!~*'()");
        assert_eq!(uri_component("区"), "%E5%8C%BA");
    }

    #[test]
    fn signature_headers_are_stable_for_a_fixed_clock() {
        let (yy, stamp, signature) = headers("/v1/api/user/info?biz_id=3", "", 1_700_000_000_123);
        assert_eq!(yy.len(), 32);
        assert_eq!(stamp, "1700000000");
        assert_eq!(signature.len(), 32);
        assert_ne!(yy, signature);
    }

    #[test]
    fn body_changes_the_signature() {
        let (_, _, empty) = headers("/x", "", 1_700_000_000_000);
        let (_, _, filled) = headers("/x", "{}", 1_700_000_000_000);
        assert_ne!(empty, filled);
    }
}
