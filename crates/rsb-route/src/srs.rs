//! sing-box binary rule-set (`.srs`) reader.

use anyhow::{bail, Context, Result};
use flate2::read::ZlibDecoder;
use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const MAGIC: [u8; 3] = [0x53, 0x52, 0x53];
const RULE_ITEM_DOMAIN: u8 = 2;
const RULE_ITEM_DOMAIN_KEYWORD: u8 = 3;
const RULE_ITEM_IPCIDR: u8 = 6;
const RULE_ITEM_FINAL: u8 = 0xFF;

#[derive(Default)]
pub struct SrsCompiled {
    pub domains: Vec<String>,
    pub domain_suffixes: Vec<String>,
    pub domain_keywords: Vec<String>,
    pub ip_cidrs: Vec<String>,
    pub ip_ranges: Vec<(IpAddr, IpAddr)>,
}

pub fn parse_srs(data: &[u8]) -> Result<SrsCompiled> {
    if data.len() < 5 || data[..3] != MAGIC {
        bail!("invalid sing-box rule-set file");
    }
    let _version = data[3];
    let mut decoder = ZlibDecoder::new(Cursor::new(&data[4..]));
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .context("decompress srs")?;
    let mut reader = Cursor::new(decompressed);
    let rule_count = read_uvarint(&mut reader)? as usize;
    let mut out = SrsCompiled::default();
    for _ in 0..rule_count {
        let rule_type = read_u8(&mut reader)?;
        match rule_type {
            0 => parse_default_rule(&mut reader, &mut out)?,
            1 => parse_logical_rule(&mut reader, &mut out)?,
            other => bail!("unknown srs rule type: {other}"),
        }
    }
    Ok(out)
}

fn parse_logical_rule(reader: &mut Cursor<Vec<u8>>, out: &mut SrsCompiled) -> Result<()> {
    let _mode = read_u8(reader)?;
    let count = read_uvarint(reader)? as usize;
    for _ in 0..count {
        let rule_type = read_u8(reader)?;
        match rule_type {
            0 => parse_default_rule(reader, out)?,
            1 => parse_logical_rule(reader, out)?,
            other => bail!("unknown nested srs rule type: {other}"),
        }
    }
    Ok(())
}

fn parse_default_rule(reader: &mut Cursor<Vec<u8>>, out: &mut SrsCompiled) -> Result<()> {
    loop {
        let item = read_u8(reader)?;
        match item {
            RULE_ITEM_DOMAIN => {
                let (domains, suffixes) = read_domain_matcher(reader)?;
                out.domains.extend(domains);
                out.domain_suffixes.extend(suffixes);
            }
            RULE_ITEM_DOMAIN_KEYWORD => {
                out.domain_keywords.extend(read_string_list(reader)?);
            }
            RULE_ITEM_IPCIDR => {
                let ranges = read_ip_set(reader)?;
                for (from, to) in ranges {
                    if from == to {
                        if let IpAddr::V4(v4) = from {
                            out.ip_cidrs.push(format!("{v4}/32"));
                        } else if let IpAddr::V6(v6) = from {
                            out.ip_cidrs.push(format!("{v6}/128"));
                        }
                    } else {
                        out.ip_ranges.push((from, to));
                    }
                }
            }
            RULE_ITEM_FINAL => {
                let _invert = read_u8(reader)?;
                break;
            }
            other => skip_rule_item(reader, other)?,
        }
    }
    Ok(())
}

fn skip_rule_item(reader: &mut Cursor<Vec<u8>>, item: u8) -> Result<()> {
    match item {
        0 | 7 | 9 => {
            let _ = read_u16_list(reader)?;
        }
        1 | 3 | 4 | 8 | 10 | 11 | 12 | 13 | 14 | 15 | 17 | 23 => {
            let _ = read_string_list(reader)?;
        }
        2 | 16 => {
            let _ = read_domain_matcher(reader)?;
        }
        5 | 6 => {
            let _ = read_ip_set(reader)?;
        }
        18 => {
            let len = read_uvarint(reader)? as usize;
            if len > 0 {
                let mut buf = vec![0u8; len];
                std::io::Read::read_exact(reader, &mut buf)?;
            }
        }
        19 | 20 => {}
        21 | 22 => skip_prefix_map(reader)?,
        other => {
            tracing::debug!(item = other, "skip unknown srs rule item");
        }
    }
    Ok(())
}

fn skip_prefix_map(reader: &mut Cursor<Vec<u8>>) -> Result<()> {
    let size = read_uvarint(reader)? as usize;
    for _ in 0..size {
        let _key = read_u8(reader)?;
        let prefix_count = read_uvarint(reader)? as usize;
        for _ in 0..prefix_count {
            let _ = read_ip_prefix(reader)?;
        }
    }
    Ok(())
}

fn read_ip_prefix(reader: &mut Cursor<Vec<u8>>) -> Result<()> {
    let _addr = read_ip(reader)?;
    let _prefix = read_u8(reader)?;
    Ok(())
}

fn read_u16_list(reader: &mut Cursor<Vec<u8>>) -> Result<Vec<u16>> {
    let count = read_uvarint(reader)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mut b = [0u8; 2];
        std::io::Read::read_exact(reader, &mut b)?;
        out.push(u16::from_be_bytes(b));
    }
    Ok(out)
}

fn read_domain_matcher(reader: &mut Cursor<Vec<u8>>) -> Result<(Vec<String>, Vec<String>)> {
    let _reserved = read_u8(reader)?;
    let leaves = read_u64_list(reader)?;
    let label_bitmap = read_u64_list(reader)?;
    let label_len = read_uvarint(reader)? as usize;
    let mut labels = vec![0u8; label_len];
    std::io::Read::read_exact(reader, &mut labels)?;
    let keys = succinct_keys(&leaves, &label_bitmap, &labels);
    let mut domains = Vec::new();
    let mut suffixes = Vec::new();
    for key in keys {
        if key.is_empty() {
            continue;
        }
        let reversed = reverse_domain(&key);
        match reversed.as_bytes().first() {
            Some(b'\r') => suffixes.push(reversed[1..].to_string()),
            Some(b'\n') => suffixes.push(reversed[1..].to_string()),
            _ => domains.push(reversed),
        }
    }
    Ok((domains, suffixes))
}

fn succinct_keys(leaves: &[u64], label_bitmap: &[u64], labels: &[u8]) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    fn traverse(
        node_id: usize,
        mut bm_idx: usize,
        leaves: &[u64],
        label_bitmap: &[u64],
        labels: &[u8],
        labels_base: &[u8],
        current: &mut Vec<u8>,
        result: &mut Vec<String>,
    ) {
        if get_bit(leaves, node_id) != 0 {
            result.push(String::from_utf8_lossy(current).into_owned());
        }
        loop {
            if get_bit(label_bitmap, bm_idx) != 0 {
                return;
            }
            let next_label = labels[bm_idx - node_id];
            current.push(next_label);
            let next_node = count_zeros(label_bitmap, bm_idx + 1);
            let next_bm = select_ith_one(label_bitmap, next_node - 1) + 1;
            traverse(
                next_node,
                next_bm,
                leaves,
                label_bitmap,
                labels,
                labels_base,
                current,
                result,
            );
            current.pop();
            bm_idx += 1;
        }
    }
    traverse(
        0,
        0,
        leaves,
        label_bitmap,
        labels,
        labels,
        &mut current,
        &mut result,
    );
    result
}

fn get_bit(words: &[u64], i: usize) -> u64 {
    words[i >> 6] & (1 << (i & 63))
}

fn count_zeros(bm: &[u64], i: usize) -> usize {
    let mut ones = 0usize;
    for word_i in 0..=(i >> 6) {
        let end = if word_i == i >> 6 { i & 63 } else { 64 };
        ones += (words_get(bm, word_i) & ((1u64 << end) - 1)).count_ones() as usize;
    }
    i - ones
}

fn words_get(words: &[u64], i: usize) -> u64 {
    words.get(i).copied().unwrap_or(0)
}

fn select_ith_one(bm: &[u64], i: usize) -> usize {
    let mut seen = 0usize;
    let total_bits = bm.len() * 64;
    for bit in 0..total_bits {
        if get_bit(bm, bit) != 0 {
            if seen == i {
                return bit;
            }
            seen += 1;
        }
    }
    total_bits
}

fn reverse_domain(domain: &str) -> String {
    domain.chars().rev().collect()
}

fn read_ip_set(reader: &mut Cursor<Vec<u8>>) -> Result<Vec<(IpAddr, IpAddr)>> {
    let version = read_u8(reader)?;
    if version != 1 {
        bail!("unsupported ip set version: {version}");
    }
    let count = read_u64_be(reader)? as usize;
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
        let from = read_ip(reader)?;
        let to = read_ip(reader)?;
        ranges.push((from, to));
    }
    Ok(ranges)
}

fn read_ip(reader: &mut Cursor<Vec<u8>>) -> Result<IpAddr> {
    let len = read_uvarint(reader)? as usize;
    let mut buf = vec![0u8; len];
    std::io::Read::read_exact(reader, &mut buf)?;
    match len {
        4 => Ok(IpAddr::V4(Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]))),
        16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&buf);
            Ok(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => bail!("invalid ip length {len}"),
    }
}

fn read_string_list(reader: &mut Cursor<Vec<u8>>) -> Result<Vec<String>> {
    let count = read_uvarint(reader)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_uvarint(reader)? as usize;
        let mut buf = vec![0u8; len];
        std::io::Read::read_exact(reader, &mut buf)?;
        out.push(String::from_utf8(buf)?);
    }
    Ok(out)
}

fn read_u64_list(reader: &mut Cursor<Vec<u8>>) -> Result<Vec<u64>> {
    let count = read_uvarint(reader)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(read_u64_be(reader)?);
    }
    Ok(out)
}

fn read_u8(reader: &mut Cursor<Vec<u8>>) -> Result<u8> {
    let mut b = [0u8; 1];
    std::io::Read::read_exact(reader, &mut b)?;
    Ok(b[0])
}

fn read_u64_be(reader: &mut Cursor<Vec<u8>>) -> Result<u64> {
    let mut b = [0u8; 8];
    std::io::Read::read_exact(reader, &mut b)?;
    Ok(u64::from_be_bytes(b))
}

fn read_uvarint(reader: &mut Cursor<Vec<u8>>) -> Result<u64> {
    let mut x = 0u64;
    let mut s = 0u32;
    loop {
        let b = read_u8(reader)?;
        if b < 0x80 {
            if s > 63 || (s == 63 && b > 1) {
                bail!("uvarint overflow");
            }
            return Ok(x | (u64::from(b) << s));
        }
        x |= u64::from(b & 0x7f) << s;
        s += 7;
    }
}
