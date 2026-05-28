// ── Arbitrary-precision bignum helpers for division ──

pub(super) fn i128_to_digits(mut n: i128) -> Vec<u8> {
    if n == 0 {
        return vec![0];
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push((n % 10) as u8);
        n /= 10;
    }
    digits.reverse();
    digits
}

pub(super) fn digits_to_string(digits: &[u8]) -> String {
    if digits.is_empty() {
        return "0".to_string();
    }
    digits.iter().map(|&d| (b'0' + d) as char).collect()
}

pub(super) fn digits_compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let a_start = a.iter().position(|&d| d != 0).unwrap_or(a.len());
    let b_start = b.iter().position(|&d| d != 0).unwrap_or(b.len());
    let a_len = a.len() - a_start;
    let b_len = b.len() - b_start;
    if a_len != b_len {
        return a_len.cmp(&b_len);
    }
    a[a_start..].cmp(&b[b_start..])
}

pub(super) fn digits_subtract(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; a.len()];
    let mut borrow: i8 = 0;
    let b_offset = a.len() as isize - b.len() as isize;
    for i in (0..a.len()).rev() {
        let bi = i as isize - b_offset;
        let b_digit = if bi >= 0 && (bi as usize) < b.len() {
            b[bi as usize] as i8
        } else {
            0
        };
        let diff = a[i] as i8 - b_digit - borrow;
        if diff < 0 {
            result[i] = (diff + 10) as u8;
            borrow = 1;
        } else {
            result[i] = diff as u8;
            borrow = 0;
        }
    }
    // Trim leading zeros
    let start = result
        .iter()
        .position(|&d| d != 0)
        .unwrap_or(result.len().saturating_sub(1));
    result[start..].to_vec()
}

pub(super) fn digits_long_div(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut quotient = Vec::new();
    let mut remainder: Vec<u8> = vec![0];
    for &digit in a {
        // Shift remainder left and append digit
        if remainder.len() == 1 && remainder[0] == 0 {
            remainder = vec![digit];
        } else {
            remainder.push(digit);
        }
        // Binary search for the quotient digit (0..9)
        let mut lo: u8 = 0;
        let mut hi: u8 = 9;
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let product = digits_mul_single(b, mid);
            if digits_compare(&product, &remainder) == std::cmp::Ordering::Greater {
                hi = mid - 1;
            } else {
                lo = mid;
            }
        }
        quotient.push(lo);
        if lo > 0 {
            let product = digits_mul_single(b, lo);
            remainder = digits_subtract(&remainder, &product);
        }
    }
    // Trim leading zeros from quotient
    let start = quotient
        .iter()
        .position(|&d| d != 0)
        .unwrap_or(quotient.len().saturating_sub(1));
    quotient[start..].to_vec()
}

pub(super) fn digits_mul_single(a: &[u8], b: u8) -> Vec<u8> {
    if b == 0 {
        return vec![0];
    }
    let mut result = vec![0u8; a.len() + 1];
    let mut carry: u16 = 0;
    for i in (0..a.len()).rev() {
        let prod = a[i] as u16 * b as u16 + carry;
        result[i + 1] = (prod % 10) as u8;
        carry = prod / 10;
    }
    result[0] = carry as u8;
    let start = result
        .iter()
        .position(|&d| d != 0)
        .unwrap_or(result.len().saturating_sub(1));
    result[start..].to_vec()
}

/// Truncate a decimal string to `prec` significant digits (ROUND_HALF_EVEN)
pub(super) fn truncate_to_prec(s: &str, prec: u32) -> String {
    if prec == 0 {
        return s.to_string();
    }
    let (neg, rest) = if s.starts_with('-') {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let (int_part, frac_part) = if let Some(dot) = rest.find('.') {
        (&rest[..dot], &rest[dot + 1..])
    } else {
        (rest, "")
    };
    let all_digits: Vec<char> = format!("{}{}", int_part, frac_part).chars().collect();
    let first_sig = match all_digits.iter().position(|&c| c != '0') {
        Some(i) => i,
        None => return s.to_string(),
    };
    let sig_count = all_digits.len() - first_sig;
    if sig_count <= prec as usize {
        return s.to_string();
    }
    let keep = first_sig + prec as usize;
    // Banker's rounding on the digit at position `keep`
    let round_digit = if keep < all_digits.len() {
        all_digits[keep].to_digit(10).unwrap_or(0)
    } else {
        0
    };
    let mut kept: Vec<u8> = all_digits[..keep]
        .iter()
        .map(|c| c.to_digit(10).unwrap_or(0) as u8)
        .collect();
    let round_up = if round_digit > 5 {
        true
    } else if round_digit == 5 {
        // Check if there are any nonzero digits after
        let has_trailing = if keep + 1 < all_digits.len() {
            all_digits[keep + 1..].iter().any(|&c| c != '0')
        } else {
            false
        };
        if has_trailing {
            true
        } else {
            kept.last().map_or(false, |&d| d % 2 != 0)
        }
    } else {
        false
    };
    if round_up {
        let mut i = kept.len();
        while i > 0 {
            i -= 1;
            if kept[i] < 9 {
                kept[i] += 1;
                break;
            }
            kept[i] = 0;
            if i == 0 {
                kept.insert(0, 1);
            }
        }
    }
    // Reconstruct
    let int_len = int_part.len();
    let trunc_str: String = kept.iter().map(|&d| (b'0' + d) as char).collect();
    if frac_part.is_empty() || keep <= int_len {
        let int_digits = &trunc_str[..std::cmp::min(int_len, trunc_str.len())];
        let pad = if int_len > trunc_str.len() {
            int_len - trunc_str.len()
        } else {
            0
        };
        let padded = format!("{}{}", int_digits, "0".repeat(pad));
        if neg && padded != "0" {
            format!("-{}", padded)
        } else {
            padded
        }
    } else {
        let int_d = &trunc_str[..int_len];
        let frac_d = &trunc_str[int_len..];
        if neg {
            format!("-{}.{}", int_d, frac_d)
        } else {
            format!("{}.{}", int_d, frac_d)
        }
    }
}
