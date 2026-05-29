use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct DecryptResult {
    pub success: bool,
    pub audio_file: String,
    pub audio_name: String,
    pub format: String,
    pub cover_file: String,
}

#[derive(Debug, Deserialize)]
struct MetaInfo {
    #[serde(rename = "musicName")]
    music_name: String,
    format: String,
}

/// Read a little-endian u32 from 4 bytes
fn read_u32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Decrypt a single AES-128-ECB block
fn aes128_ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut result = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        if chunk.len() == 16 {
            let mut block = GenericArray::clone_from_slice(chunk);
            cipher.decrypt_block(&mut block);
            result.extend_from_slice(&block);
        } else {
            // Last partial block (shouldn't happen in practice)
            result.extend_from_slice(chunk);
        }
    }
    result
}

/// Extract the real key from AES-decrypted key data.
/// Compatible with three cases: PKCS#7 padding, \r delimiter, no delimiter.
fn extract_real_key(decrypted_key: &[u8]) -> Vec<u8> {
    let prefix = b"neteasecloudmusic";

    // 1. Find and strip prefix
    let key_body = if decrypted_key.starts_with(prefix) {
        &decrypted_key[prefix.len()..]
    } else if let Some(pos) = decrypted_key.windows(prefix.len()).position(|w| w == prefix) {
        &decrypted_key[pos + prefix.len()..]
    } else {
        // No prefix: take last 111 bytes as conservative approach
        let start = if decrypted_key.len() > 111 { decrypted_key.len() - 111 } else { 0 };
        &decrypted_key[start..]
    };

    if key_body.is_empty() {
        return vec![];
    }

    // 2. Check PKCS#7 padding
    let last_byte = *key_body.last().unwrap();
    if (1..=16).contains(&last_byte) {
        let pad_len = last_byte as usize;
        if key_body.len() >= pad_len
            && key_body[key_body.len() - pad_len..].iter().all(|&b| b == last_byte)
        {
            return key_body[..key_body.len() - pad_len].to_vec();
        }
    }

    // 3. Check \r delimiter (some older files)
    if let Some(pos) = key_body.iter().position(|&b| b == b'\r') {
        return key_body[..pos].to_vec();
    }

    // 4. Fallback: return as-is
    key_body.to_vec()
}

#[allow(unused_assignments)]
pub fn decrypt_ncm(filepath: &str, output_dir: &str) -> Result<DecryptResult, String> {
    let path = Path::new(filepath);
    let _file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let _file_size = fs::metadata(filepath)
        .map_err(|e| format!("无法读取文件: {}", e))?
        .len();

    // Use buffered reader for performance on large files
    let mut f = fs::File::open(filepath).map_err(|e| format!("无法打开文件: {}", e))?;

    // Skip 8 bytes header
    f.seek(SeekFrom::Start(8)).map_err(|e| format!("读取失败: {}", e))?;

    // Read 2 bytes (gap)
    let mut gap = [0u8; 2];
    f.read_exact(&mut gap).map_err(|e| format!("读取失败: {}", e))?;

    // Key length (4 bytes)
    let mut key_len_buf = [0u8; 4];
    f.read_exact(&mut key_len_buf).map_err(|e| format!("读取失败: {}", e))?;
    let key_len = read_u32(&key_len_buf) as usize;

    // Key data
    let mut key_data = vec![0u8; key_len];
    f.read_exact(&mut key_data).map_err(|e| format!("读取失败: {}", e))?;

    // XOR with 0x64
    for b in &mut key_data {
        *b ^= 0x64;
    }

    // AES-128-ECB decrypt key data
    let ncm_key1: [u8; 16] = *b"hzHRAmso5kInbaxW";
    let decrypted_key = aes128_ecb_decrypt(&ncm_key1, &key_data);

    // Extract real key
    let real_key = extract_real_key(&decrypted_key);

    // Metadata length (4 bytes)
    let mut meta_len_buf = [0u8; 4];
    f.read_exact(&mut meta_len_buf).map_err(|e| format!("读取失败: {}", e))?;
    let meta_len = read_u32(&meta_len_buf) as usize;

    // Metadata
    let mut meta_data = vec![0u8; meta_len];
    f.read_exact(&mut meta_data).map_err(|e| format!("读取失败: {}", e))?;

    // XOR with 0x63
    for b in &mut meta_data {
        *b ^= 0x63;
    }

    // Find ':' and take everything after, then base64 decode
    let colon_pos = meta_data
        .iter()
        .position(|&b| b == b':')
        .ok_or("元数据格式错误：未找到 ':'")?;
    let json_part = &meta_data[colon_pos + 1..];

    let decoded_meta = base64::engine::general_purpose::STANDARD
        .decode(json_part)
        .map_err(|e| format!("Base64 解码失败: {}", e))?;

    // AES-128-ECB decrypt metadata
    let ncm_key2: [u8; 16] = *b"#14ljk_!\\]&0U<'(";
    let decrypted_meta = aes128_ecb_decrypt(&ncm_key2, &decoded_meta);

    // Find JSON between first ':' and last '}'
    let json_start = decrypted_meta
        .iter()
        .position(|&b| b == b':')
        .ok_or("元数据解密失败：未找到 ':'")?
        + 1;
    let json_end = decrypted_meta
        .iter()
        .rposition(|&b| b == b'}')
        .ok_or("元数据解密失败：未找到 '}'")?
        + 1;

    let json_str = std::str::from_utf8(&decrypted_meta[json_start..json_end])
        .map_err(|e| format!("JSON 解析失败: {}", e))?;

    let meta_info: MetaInfo =
        serde_json::from_str(json_str).map_err(|e| format!("JSON 解析失败: {}", e))?;

    // Build output audio name (remove '/' from name for safety)
    let audio_name_raw = format!("{}.{}", meta_info.music_name, meta_info.format);
    let audio_name = audio_name_raw.replace('/', "");
    let audio_path = Path::new(output_dir).join(&audio_name);
    let cover_path = Path::new(output_dir).join("cover.jpg");

    // Skip 4 bytes (CRC?)
    f.seek(SeekFrom::Current(4)).map_err(|e| format!("读取失败: {}", e))?;

    // Skip 5 bytes
    let mut skip5 = [0u8; 5];
    f.read_exact(&mut skip5).map_err(|e| format!("读取失败: {}", e))?;

    // Image length
    let mut img_len_buf = [0u8; 4];
    f.read_exact(&mut img_len_buf).map_err(|e| format!("读取失败: {}", e))?;
    let img_len = read_u32(&img_len_buf) as usize;

    // Image data
    let mut img_data = vec![0u8; img_len];
    f.read_exact(&mut img_data).map_err(|e| format!("读取失败: {}", e))?;

    // Save cover image
    fs::write(&cover_path, &img_data)
        .map_err(|e| format!("写入封面失败: {}", e))?;

    // Build RC4-like key box (KSA)
    let key_bytes = &real_key;
    let mut key_box: Vec<u8> = (0..=255).collect();
    let mut c: u8 = 0;
    let mut last_byte: u8 = 0;
    let mut key_offset: usize = 0;

    for i in 0..256usize {
        let swap = key_box[i];
        c = swap.wrapping_add(last_byte).wrapping_add(key_bytes[key_offset]);
        key_offset += 1;
        if key_offset >= key_bytes.len() {
            key_offset = 0;
        }
        key_box[i] = key_box[c as usize];
        key_box[c as usize] = swap;
        last_byte = c;
    }

    // Decrypt audio data
    let mut audio_out = fs::File::create(&audio_path)
        .map_err(|e| format!("创建输出文件失败: {}", e))?;

    let mut buffer = vec![0u8; 0x140000]; // 1.25MB chunks
    loop {
        let bytes_read = f.read(&mut buffer).map_err(|e| format!("读取音频数据失败: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        let chunk = &mut buffer[..bytes_read];
        for i in 0..chunk.len() {
            let j = ((i + 1) & 0xff) as u8;
            let a = key_box[j as usize] as usize;
            let inner = (a + j as usize) & 0xff;
            let b = key_box[inner] as usize;
            let outer = (a + b) & 0xff;
            chunk[i] ^= key_box[outer];
        }
        audio_out.write_all(chunk).map_err(|e| format!("写入音频数据失败: {}", e))?;
    }

    Ok(DecryptResult {
        success: true,
        audio_file: audio_path.to_string_lossy().to_string(),
        audio_name,
        format: meta_info.format,
        cover_file: cover_path.to_string_lossy().to_string(),
    })
}
