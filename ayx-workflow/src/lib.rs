use std::fs;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use roxmltree::Document;
use serde::Serialize;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use snap::raw::Decoder as SnapDecoder;
use walkdir::WalkDir;
use zip::read::ZipArchive;
use zip::write::FileOptions;
use zip::ZipWriter;

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowReplacement {
    pub find: String,
    pub replace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowIssue {
    pub path: String,
    pub issue: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowRules {
    pub replacements: Vec<WorkflowReplacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowMatch {
    pub path: String,
    pub matches: Vec<String>,
}

#[derive(Debug, Clone)]
struct MetaInfoField {
    name: String,
    data_type: String,
    size: usize,
}

#[derive(Debug, Clone)]
enum YxdbValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl YxdbValue {
    fn to_json(&self) -> Value {
        match self {
            YxdbValue::Null => Value::Null,
            YxdbValue::Bool(v) => json!(v),
            YxdbValue::I64(v) => json!(v),
            YxdbValue::F64(v) => json!(v),
            YxdbValue::String(v) => json!(v),
            YxdbValue::Bytes(v) => json!(base64_encode(v)),
        }
    }
}

struct ByteCursor {
    data: Vec<u8>,
    pos: usize,
}

struct SliceCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.pos + 1 > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u16_le(&mut self) -> Result<u16> {
        if self.pos + 2 > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let v = u16::from_le_bytes(self.data[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    fn read_u32_le(&mut self) -> Result<u32> {
        if self.pos + 4 > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    fn read_i32_le(&mut self) -> Result<i32> {
        Ok(self.read_u32_le()? as i32)
    }

    fn read_u64_le(&mut self) -> Result<u64> {
        if self.pos + 8 > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let v = u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    fn read_i64_le(&mut self) -> Result<i64> {
        Ok(self.read_u64_le()? as i64)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.pos + len > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.data[start..start + len])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YxdbFlavor {
    E1,
    E2,
}

#[derive(Debug, Clone)]
struct E2PacketIndexEntry {
    file_offset: u64,
    record_count: u32,
}

impl ByteCursor {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u32_le(&mut self) -> Result<u32> {
        if self.pos + 4 > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let value = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(value)
    }

    fn read_exact(&mut self, len: usize) -> Result<&[u8]> {
        if self.pos + len > self.data.len() {
            bail!("unexpected end of YXDB data");
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.data[start..start + len])
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn workflow_kind(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_ascii_lowercase())
    {
        Some(ext) if ext == "yxmd" => "workflow",
        Some(ext) if ext == "yxmc" => "macro",
        Some(ext) if ext == "yxzp" => "package",
        Some(ext) if ext == "yxdb" => "data",
        Some(ext) if ext == "xml" => "xml",
        _ => "other",
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn render_json_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        other => other.to_string(),
    }
}

fn parse_meta_info(xml: &str) -> Result<Vec<MetaInfoField>> {
    let doc = roxmltree::Document::parse(xml).context("YXDB metadata is invalid")?;
    let record_info = doc
        .descendants()
        .find(|node| node.has_tag_name("RecordInfo"))
        .ok_or_else(|| anyhow::anyhow!("YXDB metadata is invalid"))?;
    let mut fields = Vec::new();
    for field in record_info
        .children()
        .filter(|node| node.has_tag_name("Field"))
    {
        let name = field
            .attribute("name")
            .ok_or_else(|| anyhow::anyhow!("YXDB metadata is invalid"))?
            .to_string();
        let data_type = field
            .attribute("type")
            .ok_or_else(|| anyhow::anyhow!("YXDB metadata is invalid"))?
            .to_string();
        let size = field
            .attribute("size")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        fields.push(MetaInfoField {
            name,
            data_type,
            size,
        });
    }
    Ok(fields)
}

fn parse_e2_metadata(xml: &str) -> Result<Vec<MetaInfoField>> {
    parse_meta_info(xml)
}

fn detect_yxdb_flavor(header: &[u8; 512]) -> Result<YxdbFlavor> {
    let desc = String::from_utf8_lossy(&header[..64]);
    let file_id = u32::from_le_bytes(header[64..68].try_into().unwrap());
    if desc.starts_with("Alteryx e2 Database file") || file_id == 0x0044_0208 {
        return Ok(YxdbFlavor::E2);
    }
    if desc.starts_with("Alteryx Database File") {
        return Ok(YxdbFlavor::E1);
    }
    bail!("file is not a valid YXDB format");
}

fn parse_e2_header(file: &[u8]) -> Result<(usize, Vec<MetaInfoField>)> {
    if file.len() < 100 {
        bail!("E2 YXDB file too small");
    }
    let meta_len = u32::from_le_bytes(file[96..100].try_into().unwrap()) as usize;
    let meta_start = 100usize;
    if meta_start + meta_len > file.len() {
        bail!("E2 YXDB metadata extends beyond file");
    }
    let xml = std::str::from_utf8(&file[meta_start..meta_start + meta_len])
        .context("E2 YXDB metadata is not utf-8")?;
    let fields = parse_e2_metadata(xml)?;
    Ok((meta_len, fields))
}

fn parse_e2_footer(file: &[u8]) -> Result<(i64, Vec<E2PacketIndexEntry>, u64)> {
    if file.len() < 29 {
        bail!("E2 YXDB file too small");
    }
    let magic = u32::from_le_bytes(file[file.len() - 4..].try_into().unwrap());
    if magic != 0x3245_5859 {
        bail!("E2 YXDB footer magic not found");
    }
    let packet_count =
        i64::from_le_bytes(file[file.len() - 20..file.len() - 12].try_into().unwrap());
    let record_count =
        i64::from_le_bytes(file[file.len() - 12..file.len() - 4].try_into().unwrap());
    let footer_len = 29usize + 12usize * packet_count as usize;
    if file.len() < footer_len {
        bail!("E2 YXDB footer truncated");
    }
    let footer_start = file.len() - footer_len;
    let mut cur = SliceCursor::new(&file[footer_start..]);
    let block_type = cur.read_u8()?;
    if block_type != 0 {
        bail!("E2 YXDB footer block type mismatch");
    }
    let _spatial_idx_pos = cur.read_i64_le()?;
    let mut packets = Vec::with_capacity(packet_count as usize);
    for _ in 0..packet_count {
        let file_offset = cur.read_i64_le()? as u64;
        let record_count = cur.read_i32_le()? as u32;
        packets.push(E2PacketIndexEntry {
            file_offset,
            record_count,
        });
    }
    let counted_packet_count = cur.read_i64_le()?;
    let counted_record_count = cur.read_i64_le()?;
    if counted_packet_count != packet_count || counted_record_count != record_count {
        bail!("E2 YXDB footer counts mismatch");
    }
    Ok((record_count, packets, footer_start as u64))
}

fn snappy_decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = SnapDecoder::new();
    decoder
        .decompress_vec(bytes)
        .map_err(|e| anyhow::anyhow!("snappy decode failed: {e}"))
}

fn read_e2_packet(file: &[u8], entry: &E2PacketIndexEntry) -> Result<Vec<u8>> {
    let start = entry.file_offset as usize;
    if start + 5 > file.len() {
        bail!("E2 YXDB packet offset out of range");
    }
    let mut cur = SliceCursor::new(&file[start..]);
    let block_type = cur.read_u8()?;
    if block_type != 2 {
        bail!("E2 YXDB packet block type mismatch");
    }
    let compressed_size = cur.read_u32_le()? as usize;
    let payload = cur.read_exact(compressed_size)?;
    if payload.is_empty() {
        bail!("E2 YXDB packet payload is empty");
    }
    match payload[0] {
        0 => Ok(payload[1..].to_vec()),
        10 => snappy_decompress(&payload[1..]),
        11 => bail!("unsupported E2 YXDB packet compression type 11"),
        other => bail!("unsupported E2 YXDB packet compression type {other}"),
    }
}

#[derive(Debug)]
struct E2RecordPacket<'a> {
    data: &'a [u8],
    record_count: u32,
}

fn parse_e2_record_packet(buf: &[u8]) -> Result<E2RecordPacket<'_>> {
    if buf.len() < 8 {
        bail!("E2 YXDB record packet too small");
    }
    let word0 = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let data_len = (word0 & 0x00ff_ffff) as usize;
    let record_count = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    if buf.len() < 8 + data_len {
        bail!("E2 YXDB record packet truncated");
    }
    Ok(E2RecordPacket {
        data: &buf[8..8 + data_len],
        record_count,
    })
}

fn read_e2_field(cur: &mut SliceCursor<'_>) -> Result<YxdbValue> {
    let tag = cur.read_u8()?;
    if tag & 0x80 != 0 {
        let len = (tag & 0x7f) as usize;
        let bytes = cur.read_exact(len)?;
        return Ok(YxdbValue::String(
            String::from_utf8_lossy(bytes).to_string(),
        ));
    }
    let base = tag & 0x3f;
    if tag & 0x40 != 0 {
        return Ok(YxdbValue::Null);
    }
    match base {
        1 => {
            let len = cur.read_u16_le()? as usize;
            let bytes = cur.read_exact(len)?;
            Ok(YxdbValue::String(
                String::from_utf8_lossy(bytes).to_string(),
            ))
        }
        2..=4 => {
            let len = cur.read_u16_le()? as usize;
            let bytes = cur.read_exact(len)?.to_vec();
            Ok(YxdbValue::Bytes(bytes))
        }
        5 => Ok(YxdbValue::Bool(cur.read_u8()? != 0)),
        6 => Ok(YxdbValue::I64(0)),
        7 => Ok(YxdbValue::I64(cur.read_u8()? as i64)),
        8 => Ok(YxdbValue::I64(cur.read_u16_le()? as i16 as i64)),
        9 => Ok(YxdbValue::I64(cur.read_i32_le()? as i64)),
        10 => Ok(YxdbValue::I64(cur.read_i64_le()?)),
        11 => Ok(YxdbValue::F64(
            f32::from_le_bytes(cur.read_exact(4)?.try_into().unwrap()) as f64,
        )),
        12 => Ok(YxdbValue::F64(f64::from_le_bytes(
            cur.read_exact(8)?.try_into().unwrap(),
        ))),
        13 => Ok(YxdbValue::Bytes(cur.read_exact(4)?.to_vec())),
        14 => Ok(YxdbValue::Bytes(cur.read_exact(8)?.to_vec())),
        15 => Ok(YxdbValue::Bytes(cur.read_exact(4)?.to_vec())),
        17 | 18 | 19 | 25 | 27 => {
            let _offset = cur.read_u64_le()?;
            Ok(YxdbValue::Null)
        }
        20 => Ok(YxdbValue::Bool(false)),
        21 => Ok(YxdbValue::Bool(true)),
        22 => Ok(YxdbValue::Bytes(cur.read_exact(8)?.to_vec())),
        23 => Ok(YxdbValue::Bytes(cur.read_exact(4)?.to_vec())),
        24 | 26 => {
            let len = cur.read_u16_le()? as usize;
            let bytes = cur.read_exact(len)?.to_vec();
            Ok(YxdbValue::Bytes(bytes))
        }
        other => bail!("unsupported E2 raw field type {other}"),
    }
}

fn read_e2_rows(
    fields: &[MetaInfoField],
    file: &[u8],
    packets: &[E2PacketIndexEntry],
) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for entry in packets.iter() {
        let packet = read_e2_packet(file, entry)?;
        let rp = parse_e2_record_packet(&packet)?;
        if rp.record_count != entry.record_count {
            bail!("E2 YXDB packet record count mismatch");
        }
        for _ in 0..entry.record_count as usize {
            let mut record_cur = SliceCursor::new(rp.data);
            let _record_header = record_cur.read_u32_le()?;
            let mut row = serde_json::Map::new();
            for field in fields.iter() {
                let value = read_e2_field(&mut record_cur)?;
                row.insert(field.name.clone(), value.to_json());
            }
            rows.push(Value::Object(row));
        }
    }
    Ok(rows)
}

fn read_lzf_block(cursor: &mut ByteCursor) -> Result<Vec<u8>> {
    let mut block_len = cursor.read_u32_le()? as usize;
    if block_len & 0x8000_0000 != 0 {
        block_len &= 0x7fff_ffff;
        let bytes = cursor.read_exact(block_len)?.to_vec();
        return Ok(bytes);
    }
    let input = cursor.read_exact(block_len)?;
    decode_lzf(input)
}

fn decode_lzf(input: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![0u8; 0x40000];
    let mut iidx = 0usize;
    let mut oidx = 0usize;

    while iidx < input.len() {
        let ctrl = input[iidx];
        iidx += 1;

        if ctrl < 32 {
            let len = ctrl as usize + 1;
            if oidx + len > out.len() || iidx + len > input.len() {
                bail!("yxdb lzf decode failed");
            }
            out[oidx..oidx + len].copy_from_slice(&input[iidx..iidx + len]);
            oidx += len;
            iidx += len;
            continue;
        }

        let mut len = (ctrl >> 5) as usize;
        let mut reference = oidx as isize - (((ctrl & 0x1f) as isize) << 8) - 1;
        if len == 7 {
            if iidx >= input.len() {
                bail!("yxdb lzf decode failed");
            }
            len += input[iidx] as usize;
            iidx += 1;
        }
        if iidx >= input.len() {
            bail!("yxdb lzf decode failed");
        }
        reference -= input[iidx] as isize;
        iidx += 1;
        len += 2;

        while len > 0 {
            let available = oidx as isize - reference;
            if available <= 0 || oidx + len > out.len() {
                bail!("yxdb lzf decode failed");
            }
            let size = (available as usize).min(len);
            let src_start = reference as usize;
            let src_end = src_start + size;
            if src_end > out.len() {
                bail!("yxdb lzf decode failed");
            }
            let chunk: Vec<u8> = out[src_start..src_end].to_vec();
            out[oidx..oidx + size].copy_from_slice(&chunk);
            oidx += size;
            reference += size as isize;
            len -= size;
        }
    }

    out.truncate(oidx);
    Ok(out)
}

fn read_record_stream(cursor: &mut ByteCursor) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    while cursor.pos < cursor.data.len() {
        let block = read_lzf_block(cursor)?;
        data.extend_from_slice(&block);
    }
    Ok(data)
}

fn parse_bool(value: u8) -> Option<bool> {
    match value {
        1 => Some(true),
        0 => Some(false),
        2 => None,
        _ => Some(value != 0),
    }
}

fn read_fixed_record(fields: &[MetaInfoField], record: &[u8]) -> Result<Vec<(String, YxdbValue)>> {
    let mut values = Vec::with_capacity(fields.len());
    let mut start_at = 0usize;
    for field in fields {
        let name = field.name.clone();
        match field.data_type.as_str() {
            "Int16" => {
                let val = i16::from_le_bytes(record[start_at..start_at + 2].try_into().unwrap());
                let null = record[start_at + 2] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::I64(val as i64)
                    },
                ));
                start_at += 3;
            }
            "Int32" => {
                let val = i32::from_le_bytes(record[start_at..start_at + 4].try_into().unwrap());
                let null = record[start_at + 4] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::I64(val as i64)
                    },
                ));
                start_at += 5;
            }
            "Int64" => {
                let val = i64::from_le_bytes(record[start_at..start_at + 8].try_into().unwrap());
                let null = record[start_at + 8] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::I64(val)
                    },
                ));
                start_at += 9;
            }
            "Float" => {
                let val =
                    f32::from_le_bytes(record[start_at..start_at + 4].try_into().unwrap()) as f64;
                let null = record[start_at + 4] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::F64(val)
                    },
                ));
                start_at += 5;
            }
            "Double" => {
                let val = f64::from_le_bytes(record[start_at..start_at + 8].try_into().unwrap());
                let null = record[start_at + 8] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::F64(val)
                    },
                ));
                start_at += 9;
            }
            "FixedDecimal" => {
                let len = field.size;
                let null = record[start_at + len] == 1;
                let text = String::from_utf8_lossy(&record[start_at..start_at + len]).to_string();
                let val = text.trim_matches('\0').trim().parse::<f64>().ok();
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        val.map(YxdbValue::F64).unwrap_or(YxdbValue::String(text))
                    },
                ));
                start_at += len + 1;
            }
            "String" => {
                let len = field.size;
                let null = record[start_at + len] == 1;
                let text = String::from_utf8_lossy(&record[start_at..start_at + len])
                    .trim_end_matches('\0')
                    .to_string();
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::String(text)
                    },
                ));
                start_at += len + 1;
            }
            "WString" => {
                let len = field.size * 2;
                let null = record[start_at + len] == 1;
                let mut utf16 = Vec::new();
                for chunk in record[start_at..start_at + len].chunks_exact(2) {
                    let word = u16::from_le_bytes([chunk[0], chunk[1]]);
                    if word == 0 {
                        break;
                    }
                    utf16.push(word);
                }
                let text = String::from_utf16_lossy(&utf16);
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::String(text)
                    },
                ));
                start_at += len + 1;
            }
            "V_String" => {
                let fixed = u32::from_le_bytes(record[start_at..start_at + 4].try_into().unwrap());
                let val = parse_blob(record, start_at, fixed, false, &name)?;
                values.push((name, val));
                start_at += 4;
            }
            "V_WString" => {
                let fixed = u32::from_le_bytes(record[start_at..start_at + 4].try_into().unwrap());
                let val = parse_blob(record, start_at, fixed, true, &name)?;
                values.push((name, val));
                start_at += 4;
            }
            "Date" => {
                let text = String::from_utf8_lossy(&record[start_at..start_at + 10]).to_string();
                let null = record[start_at + 10] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::String(text)
                    },
                ));
                start_at += 11;
            }
            "DateTime" => {
                let text = String::from_utf8_lossy(&record[start_at..start_at + 19]).to_string();
                let null = record[start_at + 19] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::String(text)
                    },
                ));
                start_at += 20;
            }
            "Bool" => {
                let val = parse_bool(record[start_at]);
                values.push((name, val.map(YxdbValue::Bool).unwrap_or(YxdbValue::Null)));
                start_at += 1;
            }
            "Byte" => {
                let null = record[start_at + 1] == 1;
                values.push((
                    name,
                    if null {
                        YxdbValue::Null
                    } else {
                        YxdbValue::I64(record[start_at] as i64)
                    },
                ));
                start_at += 2;
            }
            "Blob" | "SpatialObj" => {
                let fixed = u32::from_le_bytes(record[start_at..start_at + 4].try_into().unwrap());
                let val = parse_blob(record, start_at, fixed, false, &name)?;
                values.push((name, val));
                start_at += 4;
            }
            _ => bail!("unsupported YXDB field type '{}'", field.data_type),
        }
    }
    Ok(values)
}

fn parse_blob(
    record: &[u8],
    start: usize,
    fixed_portion: u32,
    wstring: bool,
    field_name: &str,
) -> Result<YxdbValue> {
    if fixed_portion == 0 {
        return Ok(if wstring {
            YxdbValue::String(String::new())
        } else {
            YxdbValue::Bytes(Vec::new())
        });
    }
    if fixed_portion == 1 {
        return Ok(YxdbValue::Null);
    }
    if fixed_portion & 0x8000_0000 == 0 && fixed_portion & 0x3000_0000 != 0 {
        let length = (fixed_portion >> 28) as usize;
        let inline = fixed_portion.to_le_bytes();
        let bytes = &inline[..length.min(3)];
        return if wstring {
            let utf16 = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|v| *v != 0)
                .collect::<Vec<_>>();
            Ok(YxdbValue::String(String::from_utf16_lossy(&utf16)))
        } else {
            Ok(YxdbValue::String(
                String::from_utf8_lossy(bytes).to_string(),
            ))
        };
    }
    let block_start = start + (fixed_portion & 0x7fff_ffff) as usize;
    if block_start + 4 > record.len() {
        bail!("yxdb var-data offset out of range for field '{}' at offset {} in record len {} (pointer {})", field_name, start, record.len(), fixed_portion);
    }
    let first_byte = record[block_start];
    if first_byte & 1 == 1 {
        let len = (first_byte >> 1) as usize;
        let end = block_start + 1 + len;
        if end > record.len() {
            bail!("yxdb var-data out of range for field '{}' at offset {} in record len {} (pointer {})", field_name, start, record.len(), fixed_portion);
        }
        let bytes = record[block_start + 1..end].to_vec();
        return if wstring {
            Ok(YxdbValue::String(String::from_utf16_lossy(
                &bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|v| *v != 0)
                    .collect::<Vec<_>>(),
            )))
        } else {
            Ok(YxdbValue::String(
                String::from_utf8_lossy(&bytes).to_string(),
            ))
        };
    }
    let blob_len = u32::from_le_bytes(record[block_start..block_start + 4].try_into().unwrap());
    let len = (blob_len / 2) as usize;
    let end = block_start + 4 + len;
    if end > record.len() {
        bail!(
            "yxdb var-data out of range for field '{}' at offset {} in record len {} (pointer {})",
            field_name,
            start,
            record.len(),
            fixed_portion
        );
    }
    let bytes = record[block_start + 4..end].to_vec();
    Ok(if wstring {
        let utf16 = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|v| *v != 0)
            .collect::<Vec<_>>();
        YxdbValue::String(String::from_utf16_lossy(&utf16))
    } else {
        YxdbValue::Bytes(bytes)
    })
}
pub fn read_yxdb(path: &Path, csv_output: Option<&Path>) -> Result<Value> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
    let mut header = [0u8; 512];
    file.read_exact(&mut header)
        .with_context(|| format!("failed to read YXDB header '{}'", path.display()))?;

    let flavor = detect_yxdb_flavor(&header)?;
    let mut rows = Vec::new();
    let fields: Vec<MetaInfoField>;

    if flavor == YxdbFlavor::E2 {
        let mut rest = Vec::new();
        file.read_to_end(&mut rest)
            .with_context(|| format!("failed to read YXDB body '{}'", path.display()))?;
        let mut full = header.to_vec();
        full.extend_from_slice(&rest);
        let (meta_len, parsed_fields) = parse_e2_header(&full)?;
        fields = parsed_fields;
        let record_start = 100usize + meta_len;
        let (footer_record_count, packets, footer_start) = parse_e2_footer(&full)?;
        let _ = footer_record_count;
        if record_start > footer_start as usize {
            bail!("E2 YXDB metadata overlaps footer");
        }
        rows = read_e2_rows(&fields, &full, &packets)?;
    } else {
        let meta_size = u32::from_le_bytes(header[80..84].try_into().unwrap()) as usize;
        let num_records = u32::from_le_bytes(header[104..108].try_into().unwrap()) as usize;
        let meta_len = meta_size
            .checked_mul(2)
            .and_then(|v| v.checked_sub(2))
            .ok_or_else(|| anyhow::anyhow!("YXDB metadata is invalid"))?;
        let mut meta_bytes = vec![0u8; meta_len];
        file.read_exact(&mut meta_bytes)
            .with_context(|| format!("failed to read YXDB metadata '{}'", path.display()))?;
        let mut terminator = [0u8; 2];
        file.read_exact(&mut terminator).with_context(|| {
            format!(
                "failed to read YXDB metadata terminator '{}'",
                path.display()
            )
        })?;
        let meta_xml = String::from_utf16_lossy(
            &meta_bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
        let record_block_index_pos = u64::from_le_bytes(header[96..104].try_into().unwrap());
        fields = parse_meta_info(&meta_xml)?;
        let record_data_end = record_block_index_pos;
        let record_data_start = file
            .stream_position()
            .with_context(|| format!("failed to query YXDB position '{}'", path.display()))?;
        if record_data_end < record_data_start {
            bail!("YXDB block index position precedes record data");
        }
        let record_data_len = (record_data_end - record_data_start) as usize;
        let mut rest = vec![0u8; record_data_len];
        file.read_exact(&mut rest)
            .with_context(|| format!("failed to read YXDB records '{}'", path.display()))?;
        let mut cursor = ByteCursor::new(rest);
        let stream = read_record_stream(&mut cursor)?;
        let mut record_cursor = ByteCursor::new(stream);
        rows.reserve(num_records);
        for _ in 0..num_records {
            let record_len = if fields.iter().any(|field| {
                matches!(
                    field.data_type.as_str(),
                    "V_String" | "V_WString" | "Blob" | "SpatialObj"
                )
            }) {
                let fixed_size = fields.iter().fold(0usize, |acc, field| {
                    acc + match field.data_type.as_str() {
                        "Int16" => 3,
                        "Int32" => 5,
                        "Int64" => 9,
                        "Float" => 5,
                        "Double" => 9,
                        "FixedDecimal" => field.size + 1,
                        "String" => field.size + 1,
                        "WString" => field.size * 2 + 1,
                        "V_String" | "V_WString" => 4,
                        "Date" => 11,
                        "DateTime" => 20,
                        "Bool" => 1,
                        "Byte" => 2,
                        "Blob" | "SpatialObj" => 4,
                        other => panic!("unsupported YXDB field type {}", other),
                    }
                });
                let fixed_and_len = fixed_size + 4;
                let prefix = record_cursor.read_exact(fixed_and_len)?.to_vec();
                let var_len =
                    u32::from_le_bytes(prefix[fixed_size..fixed_size + 4].try_into().unwrap())
                        as usize;
                let var_bytes = record_cursor.read_exact(var_len)?.to_vec();
                let mut record = prefix;
                record.extend_from_slice(&var_bytes);
                record
            } else {
                let fixed_size = fields.iter().fold(0usize, |acc, field| {
                    acc + match field.data_type.as_str() {
                        "Int16" => 3,
                        "Int32" => 5,
                        "Int64" => 9,
                        "Float" => 5,
                        "Double" => 9,
                        "FixedDecimal" => field.size + 1,
                        "String" => field.size + 1,
                        "WString" => field.size * 2 + 1,
                        "V_String" | "V_WString" => 4,
                        "Date" => 11,
                        "DateTime" => 20,
                        "Bool" => 1,
                        "Byte" => 2,
                        "Blob" | "SpatialObj" => 4,
                        other => panic!("unsupported YXDB field type {}", other),
                    }
                });
                record_cursor.read_exact(fixed_size)?.to_vec()
            };
            let parsed = read_fixed_record(&fields, &record_len)?;
            let mut row = serde_json::Map::new();
            for (name, value) in parsed {
                row.insert(name, value.to_json());
            }
            rows.push(Value::Object(row));
        }
    }

    if let Some(csv_path) = csv_output {
        if let Some(parent) = csv_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create '{}'", parent.display()))?;
        }
        let mut writer = fs::File::create(csv_path)
            .with_context(|| format!("failed to create '{}'", csv_path.display()))?;
        let headers: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
        writer.write_all(
            format!(
                "{}\n",
                headers
                    .iter()
                    .map(|h| csv_escape(h))
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .as_bytes(),
        )?;
        for row in &rows {
            let obj = row.as_object().unwrap();
            let line = headers
                .iter()
                .map(|h| csv_escape(obj.get(h).map(render_json_cell).as_deref().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(",");
            writer.write_all(format!("{}\n", line).as_bytes())?;
        }
    }

    let field_names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
    Ok(json!({
        "path": path.display().to_string(),
        "field_count": field_names.len(),
        "fields": field_names,
        "row_count": rows.len(),
        "rows": rows,
        "csv_written": csv_output.is_some(),
        "csv_path": csv_output.map(|p| p.display().to_string()),
    }))
}

fn is_xml_like(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if ext == "yxmd" || ext == "yxmc" || ext == "xml"
    )
}

fn is_workflow_artifact(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if ext == "yxmd" || ext == "yxmc" || ext == "yxzp" || ext == "yxdb" || ext == "xml"
    )
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn validate_xml_text(text: &str) -> Result<()> {
    let normalized = normalize_text(text);
    Document::parse(&normalized).context("failed to parse workflow xml")?;
    Ok(())
}

fn apply_replacements(text: &str, replacements: &[WorkflowReplacement]) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut matches = Vec::new();
    for replacement in replacements {
        if out.contains(&replacement.find) {
            matches.push(replacement.find.clone());
            out = out.replace(&replacement.find, &replacement.replace);
        }
    }
    (out, matches)
}

fn scan_text(text: &str, replacements: &[WorkflowReplacement]) -> Vec<String> {
    let mut matches = Vec::new();
    for replacement in replacements {
        if text.contains(&replacement.find) {
            matches.push(replacement.find.clone());
        }
    }
    matches
}

pub fn load_rules(path: &Path) -> Result<WorkflowRules> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow rules '{}'", path.display()))?;
    let yaml: YamlValue = serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse workflow rules '{}'", path.display()))?;
    let replacements = yaml
        .get("replacements")
        .and_then(|value| value.as_sequence())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workflow rules '{}' missing replacements array",
                path.display()
            )
        })?
        .iter()
        .map(|item| {
            let find = item
                .get("find")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "workflow rules '{}' replacement missing find",
                        path.display()
                    )
                })?;
            let replace = item
                .get("replace")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "workflow rules '{}' replacement missing replace",
                        path.display()
                    )
                })?;
            Ok(WorkflowReplacement {
                find: find.to_string(),
                replace: replace.to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(WorkflowRules { replacements })
}

fn scan_path(path: &Path, replacements: &[WorkflowReplacement]) -> Result<Vec<WorkflowMatch>> {
    if path.is_dir() {
        let mut results = Vec::new();
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_workflow_artifact(entry.path()) {
                let text = if entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|s| s.eq_ignore_ascii_case("yxzp"))
                    .unwrap_or(false)
                {
                    continue;
                } else {
                    read_text(entry.path())?
                };
                let matches = scan_text(&text, replacements);
                if !matches.is_empty() {
                    results.push(WorkflowMatch {
                        path: entry.path().display().to_string(),
                        matches,
                    });
                }
            }
        }
        return Ok(results);
    }

    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("failed to read zip archive '{}'", path.display()))?;
        let mut results = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_string();
            let entry_path = Path::new(&name);
            if is_xml_like(entry_path) {
                let mut buf = String::new();
                entry.read_to_string(&mut buf)?;
                let matches = scan_text(&buf, replacements);
                if !matches.is_empty() {
                    results.push(WorkflowMatch {
                        path: name,
                        matches,
                    });
                }
            }
        }
        return Ok(results);
    }

    let text = read_text(path)?;
    let matches = scan_text(&text, replacements);
    Ok(if matches.is_empty() {
        Vec::new()
    } else {
        vec![WorkflowMatch {
            path: path.display().to_string(),
            matches,
        }]
    })
}

pub fn scan(path: &Path, replacements: &[WorkflowReplacement]) -> Result<Value> {
    let matches = scan_path(path, replacements)?;
    Ok(json!({
        "path": path.display().to_string(),
        "match_count": matches.len(),
        "matches": matches,
    }))
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok(text),
        Err(_) => Ok(String::from_utf8_lossy(&bytes).to_string()),
    }
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("failed to write '{}'", path.display()))
}

fn inspect_file(path: &Path) -> Result<Value> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat '{}'", path.display()))?;
    let kind = workflow_kind(path);
    let content = if is_xml_like(path) {
        let text = read_text(path)?;
        let valid = validate_xml_text(&text).is_ok();
        json!({
            "xml_valid": valid,
            "contains": {
                "workflow": text.contains("<Nodes") || text.contains("<Node"),
                "macro": text.contains("Macro") || text.contains("<EngineSettings"),
            }
        })
    } else {
        json!({})
    };

    Ok(json!({
        "path": path.display().to_string(),
        "kind": kind,
        "size_bytes": metadata.len(),
        "xml": content,
    }))
}

fn inspect_package(path: &Path) -> Result<Value> {
    let file =
        fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive '{}'", path.display()))?;
    let mut entries = Vec::new();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let entry_path = Path::new(&name);
        let kind = workflow_kind(entry_path).to_string();
        *counts.entry(kind).or_insert(0) += 1;
        entries.push(json!({
            "name": name,
            "kind": workflow_kind(entry_path),
            "size_bytes": entry.size(),
        }));
    }
    Ok(json!({
        "path": path.display().to_string(),
        "kind": "package",
        "entry_count": entries.len(),
        "kind_counts": counts,
        "entries": entries,
    }))
}

pub fn inspect(path: &Path) -> Result<Value> {
    if path.is_dir() {
        let mut items = Vec::new();
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_workflow_artifact(entry.path()) {
                items.push(inspect_file(entry.path())?);
            }
        }
        return Ok(json!({
            "path": path.display().to_string(),
            "kind": "directory",
            "item_count": items.len(),
            "items": items,
        }));
    }

    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        return inspect_package(path);
    }

    inspect_file(path)
}

pub fn unpack_package(input: &Path, output_dir: &Path) -> Result<Value> {
    let file =
        fs::File::open(input).with_context(|| format!("failed to open '{}'", input.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive '{}'", input.display()))?;
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create '{}'", output_dir.display()))?;

    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let out_path = output_dir.join(&name);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("failed to create '{}'", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create '{}'", parent.display()))?;
        }
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create '{}'", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)?;
        entries.push(name);
    }

    Ok(json!({
        "input": input.display().to_string(),
        "output_dir": output_dir.display().to_string(),
        "entry_count": entries.len(),
        "entries": entries,
    }))
}

pub fn repackage_dir(input_dir: &Path, output_path: &Path) -> Result<Value> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let file = fs::File::create(output_path)
        .with_context(|| format!("failed to create '{}'", output_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut entry_count = 0usize;

    for entry in WalkDir::new(input_dir) {
        let entry = entry?;
        let path = entry.path();
        if path == input_dir {
            continue;
        }
        let rel = path
            .strip_prefix(input_dir)
            .with_context(|| format!("failed to strip prefix '{}'", input_dir.display()))?;
        if entry.file_type().is_dir() {
            let rel_name = format!("{}/", rel.to_string_lossy().replace('\\', "/"));
            zip.add_directory(rel_name, options)?;
            continue;
        }
        zip.start_file(rel.to_string_lossy().replace('\\', "/"), options)?;
        let mut input = fs::File::open(path)?;
        std::io::copy(&mut input, &mut zip)?;
        entry_count += 1;
    }
    zip.finish()?;

    Ok(json!({
        "input_dir": input_dir.display().to_string(),
        "output": output_path.display().to_string(),
        "entry_count": entry_count,
    }))
}

pub fn validate(path: &Path) -> Result<Value> {
    let mut issues = Vec::<WorkflowIssue>::new();
    let mut validated = Vec::new();

    if path.is_dir() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_xml_like(entry.path()) {
                let text = read_text(entry.path())?;
                match validate_xml_text(&text) {
                    Ok(()) => validated.push(entry.path().display().to_string()),
                    Err(err) => issues.push(WorkflowIssue {
                        path: entry.path().display().to_string(),
                        issue: err.to_string(),
                    }),
                }
            }
        }
    } else if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open '{}'", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("failed to read zip archive '{}'", path.display()))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_string();
            let entry_path = Path::new(&name);
            if is_xml_like(entry_path) {
                let mut buf = String::new();
                entry.read_to_string(&mut buf)?;
                match validate_xml_text(&buf) {
                    Ok(()) => validated.push(name),
                    Err(err) => issues.push(WorkflowIssue {
                        path: name,
                        issue: err.to_string(),
                    }),
                }
            }
        }
    } else if is_xml_like(path) {
        let text = read_text(path)?;
        validate_xml_text(&text)?;
        validated.push(path.display().to_string());
    } else {
        bail!("workflow validate expects a .yxmd, .yxmc, .yxzp, or directory");
    }

    Ok(json!({
        "path": path.display().to_string(),
        "validated": validated,
        "issues": issues,
        "ok": issues.is_empty(),
    }))
}

pub fn replace(
    input: &Path,
    output: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    if input.is_dir() {
        fs::create_dir_all(output)
            .with_context(|| format!("failed to create '{}'", output.display()))?;
        let mut touched = Vec::new();
        for entry in WalkDir::new(input)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_file() && is_workflow_artifact(entry.path()) {
                let rel = entry
                    .path()
                    .strip_prefix(input)
                    .with_context(|| format!("failed to strip prefix '{}'", input.display()))?;
                let out_path = output.join(rel);
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let text = read_text(entry.path())?;
                let (replaced, found) = apply_replacements(&text, replacements);
                write_text(&out_path, &replaced)?;
                touched.push(json!({
                    "path": rel.to_string_lossy(),
                    "matches": found,
                }));
            }
        }
        let validation = if validate_after {
            Some(validate(output)?)
        } else {
            None
        };
        return Ok(json!({
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "mode": "directory",
            "touched": touched,
            "validation": validation,
        }));
    }

    if input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let unpack_dir = output.with_extension("unpacked");
        if unpack_dir.exists() {
            fs::remove_dir_all(&unpack_dir)?;
        }
        unpack_package(input, &unpack_dir)?;
        let result = replace(&unpack_dir, &unpack_dir, replacements, validate_after)?;
        repackage_dir(&unpack_dir, output)?;
        return Ok(json!({
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "mode": "package",
            "unpacked_dir": unpack_dir.display().to_string(),
            "replace_result": result,
        }));
    }

    let text = read_text(input)?;
    let (replaced, found) = apply_replacements(&text, replacements);
    if validate_after && is_xml_like(input) {
        validate_xml_text(&replaced)?;
    }
    write_text(output, &replaced)?;
    Ok(json!({
        "input": input.display().to_string(),
        "output": output.display().to_string(),
        "mode": "file",
        "matches": found,
        "validated": validate_after,
    }))
}

pub fn migrate(
    input: &Path,
    output: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    replace(input, output, replacements, validate_after)
}

fn recurse_directory(
    input_dir: &Path,
    output_dir: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    if input_dir == output_dir {
        let mut touched = Vec::new();
        let mut nested = Vec::new();
        for entry in WalkDir::new(input_dir)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|s| s.eq_ignore_ascii_case("yxzp"))
                .unwrap_or(false)
            {
                let unpack_dir = path.with_extension("unpacked");
                if unpack_dir.exists() {
                    fs::remove_dir_all(&unpack_dir)?;
                }
                unpack_package(path, &unpack_dir)?;
                let nested_result =
                    recurse_directory(&unpack_dir, &unpack_dir, replacements, validate_after)?;
                repackage_dir(&unpack_dir, path)?;
                nested.push(json!({
                    "package": path.display().to_string(),
                    "result": nested_result,
                }));
                continue;
            }
            if is_workflow_artifact(path) {
                let text = read_text(path)?;
                let (replaced, found) = apply_replacements(&text, replacements);
                if validate_after && is_xml_like(path) {
                    validate_xml_text(&replaced)?;
                }
                write_text(path, &replaced)?;
                let rel = path.strip_prefix(input_dir).unwrap_or(path);
                touched.push(json!({
                    "path": rel.to_string_lossy(),
                    "matches": found,
                }));
            }
        }
        let validation = if validate_after {
            Some(validate(input_dir)?)
        } else {
            None
        };
        return Ok(json!({
            "input": input_dir.display().to_string(),
            "output": output_dir.display().to_string(),
            "mode": "directory",
            "touched": touched,
            "nested_packages": nested,
            "validation": validation,
        }));
    }

    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create '{}'", output_dir.display()))?;
    let mut touched = Vec::new();
    let mut nested = Vec::new();
    for entry in WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(input_dir)
            .with_context(|| format!("failed to strip prefix '{}'", input_dir.display()))?;
        let out_path = output_dir.join(rel);
        if entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.eq_ignore_ascii_case("yxzp"))
            .unwrap_or(false)
        {
            let nested_unpack = out_path.with_extension("unpacked");
            if nested_unpack.exists() {
                fs::remove_dir_all(&nested_unpack)?;
            }
            unpack_package(entry.path(), &nested_unpack)?;
            let nested_result =
                recurse_directory(&nested_unpack, &nested_unpack, replacements, validate_after)?;
            repackage_dir(&nested_unpack, &out_path)?;
            nested.push(json!({
                "package": rel.to_string_lossy(),
                "result": nested_result,
            }));
            continue;
        }
        if is_workflow_artifact(entry.path()) {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let text = read_text(entry.path())?;
            let (replaced, found) = apply_replacements(&text, replacements);
            if validate_after && is_xml_like(entry.path()) {
                validate_xml_text(&replaced)?;
            }
            write_text(&out_path, &replaced)?;
            touched.push(json!({
                "path": rel.to_string_lossy(),
                "matches": found,
            }));
        }
    }
    let validation = if validate_after {
        Some(validate(output_dir)?)
    } else {
        None
    };
    Ok(json!({
        "input": input_dir.display().to_string(),
        "output": output_dir.display().to_string(),
        "mode": "directory",
        "touched": touched,
        "nested_packages": nested,
        "validation": validation,
    }))
}

pub fn recurse(
    input: &Path,
    output: &Path,
    replacements: &[WorkflowReplacement],
    validate_after: bool,
) -> Result<Value> {
    if input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.eq_ignore_ascii_case("yxzp"))
        .unwrap_or(false)
    {
        let unpack_dir = output.with_extension("unpacked");
        if unpack_dir.exists() {
            fs::remove_dir_all(&unpack_dir)?;
        }
        unpack_package(input, &unpack_dir)?;
        let result = recurse_directory(&unpack_dir, &unpack_dir, replacements, validate_after)?;
        if output
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.eq_ignore_ascii_case("yxzp"))
            .unwrap_or(false)
        {
            repackage_dir(&unpack_dir, output)?;
        }
        return Ok(json!({
            "input": input.display().to_string(),
            "output": output.display().to_string(),
            "mode": "package",
            "unpacked_dir": unpack_dir.display().to_string(),
            "result": result,
        }));
    }

    if input.is_dir() {
        return recurse_directory(input, output, replacements, validate_after);
    }

    replace(input, output, replacements, validate_after)
}

pub fn package_summary(path: &Path) -> Result<Value> {
    inspect(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use snap::raw::Encoder as SnapEncoder;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::NamedTempFile;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ayx-workflow-{}-{}-{}",
            std::process::id(),
            nanos,
            name
        ))
    }

    fn e2_test_file() -> Vec<u8> {
        let meta = r#"<MetaInfo><RecordInfo><Field name="name" type="String" size="0"/><Field name="age" type="Int32" size="0"/><Field name="active" type="Bool" size="0"/></RecordInfo></MetaInfo>"#;
        let mut header = vec![0u8; 512];
        let desc = b"Alteryx e2 Database file";
        header[..desc.len()].copy_from_slice(desc);
        header[64..68].copy_from_slice(&0x0044_0208u32.to_le_bytes());
        header[68] = 1;
        header[69..73].copy_from_slice(&(4u32 * 1024 * 1024).to_le_bytes());
        header[96..100].copy_from_slice(&(meta.len() as u32).to_le_bytes());
        header[100..100 + meta.len()].copy_from_slice(meta.as_bytes());

        let mut record = Vec::new();
        record.extend_from_slice(&0u32.to_le_bytes());
        record.extend_from_slice(&1u32.to_le_bytes());
        record.push(0x85);
        record.extend_from_slice(b"Alice");
        record.push(0x09);
        record.extend_from_slice(&42i32.to_le_bytes());
        record.push(0x15);
        let data_len = record.len() - 8;
        record[0..4].copy_from_slice(&((data_len as u32) & 0x00ff_ffff).to_le_bytes());

        let mut encoder = SnapEncoder::new();
        let compressed = encoder.compress_vec(&record).unwrap();
        let mut packet = Vec::new();
        packet.push(2);
        packet.extend_from_slice(&((compressed.len() + 1) as u32).to_le_bytes());
        packet.push(10);
        packet.extend_from_slice(&compressed);

        let packet_offset = header.len() as u64;
        let mut file = header;
        file.extend_from_slice(&packet);
        let mut footer = Vec::new();
        footer.push(0);
        footer.extend_from_slice(&0i64.to_le_bytes());
        footer.extend_from_slice(&packet_offset.to_le_bytes());
        footer.extend_from_slice(&1i32.to_le_bytes());
        footer.extend_from_slice(&1i64.to_le_bytes());
        footer.extend_from_slice(&1i64.to_le_bytes());
        footer.extend_from_slice(&0x3245_5859u32.to_le_bytes());
        file.extend_from_slice(&footer);
        file
    }

    #[test]
    fn validate_xml_and_replace_text() {
        let input = temp_path("workflow.yxmd");
        write_text(
            &input,
            "<AlteryxDocument><Node>abc</Node></AlteryxDocument>",
        )
        .unwrap();
        let output = temp_path("workflow-out.yxmd");
        let result = replace(
            &input,
            &output,
            &[WorkflowReplacement {
                find: "abc".into(),
                replace: "xyz".into(),
            }],
            true,
        )
        .unwrap();
        assert!(result["matches"].as_array().unwrap().len() == 1);
        let text = read_text(&output).unwrap();
        assert!(text.contains("xyz"));
        let _ = fs::remove_file(&input);
        let _ = fs::remove_file(&output);
    }

    #[test]
    fn parse_e2_synthetic_round_trip() {
        let bytes = e2_test_file();
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        let out = tmp.path().with_extension("csv");
        let result = read_yxdb(tmp.path(), Some(&out)).unwrap();
        assert_eq!(result["field_count"].as_u64().unwrap(), 3);
        assert_eq!(result["row_count"].as_u64().unwrap(), 1);
        let csv = std::fs::read_to_string(&out).unwrap();
        assert_eq!(csv.lines().count(), 2);
    }

    #[test]
    fn inspect_xml_file() {
        let input = temp_path("workflow.yxmc");
        write_text(
            &input,
            "<AlteryxDocument><Node>abc</Node></AlteryxDocument>",
        )
        .unwrap();
        let result = inspect(&input).unwrap();
        assert_eq!(result["kind"], "macro");
        let _ = fs::remove_file(&input);
    }
}
