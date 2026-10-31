// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub mod load;
pub mod save;

#[cfg(test)]
mod tests;

pub use load::load_world;
pub use save::save_world;

pub const WORLD_FORMAT_VERSION: u32 = 2;

pub(super) fn encode_token(value: &str) -> String {
    let mut out = String::from("~WK~");
    for byte in value.as_bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(super) fn decode_token(value: &str) -> String {
    if let Some(hex) = value.strip_prefix("~WK~") {
        if hex.len() % 2 == 0 {
            let mut bytes = Vec::with_capacity(hex.len() / 2);
            for chunk in hex.as_bytes().chunks_exact(2) {
                let Ok(pair) = std::str::from_utf8(chunk) else {
                    return value.to_owned();
                };
                let Ok(byte) = u8::from_str_radix(pair, 16) else {
                    return value.to_owned();
                };
                bytes.push(byte);
            }
            if let Ok(decoded) = String::from_utf8(bytes) {
                return decoded;
            }
        }
    }
    value.to_owned()
}
