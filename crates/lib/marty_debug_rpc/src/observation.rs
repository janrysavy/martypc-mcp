//! Coherent, paused inspection: validate the entire request before any copy.
use super::*;
use marty_core::device_traits::videocard::VideoType;

pub(super) fn bytes(data: &[u8], address: usize, revision: u64) -> Value {
    json!({"address":address,"byte_count":data.len(),"data_base64":STANDARD.encode(data),
        "data_hex":data.iter().map(|b|format!("{b:02x}")).collect::<String>(),
        "sha256":digest(data),"state_revision":revision})
}
fn keys(p: &Value, allowed: &[&str], message: &'static str) -> Result<()> {
    let p = p.as_object().ok_or((message, -32602))?;
    if p.keys().any(|k| !allowed.contains(&k.as_str())) {
        return invalid(message);
    }
    Ok(())
}
fn text_layout(machine: &Machine, p: &Value) -> Result<(usize, usize, u8)> {
    keys(
        p,
        &["page", "display_address"],
        "video_text must be an object with page or display_address",
    )?;
    let card = machine
        .bus()
        .primary_video()
        .ok_or(("CGA text adapter required", -32602))?;
    if card.video_type() != VideoType::CGA || card.is_in_graphics_mode() {
        return invalid("CGA text adapter required");
    }
    let mode = card.cga_mode_control().ok_or(("CGA text adapter required", -32602))?;
    if mode & 2 != 0 {
        return invalid("CGA text adapter required");
    }
    video_mapping(machine)?;
    let columns = if mode & 1 == 0 { 40 } else { 80 };
    let active = (card.start_address() as usize * 2) & 0x3fff;
    let display = if p.get("page").is_some() {
        let page = number(&p["page"])?;
        if page >= (16384 / (columns * 25 * 2)) as u64 {
            return invalid("page outside video memory");
        }
        page as usize * columns * 25 * 2
    } else if p.get("display_address").is_some() {
        let offset = number(&p["display_address"])?;
        if offset >= 16384 {
            return invalid("display_address must be within video memory");
        }
        offset as usize
    } else {
        active
    };
    Ok((columns, display, mode))
}
fn video_mapping(machine: &Machine) -> Result<()> {
    if (0xb8000..0xbc000).any(|addr| !machine.bus().is_primary_cga_memory(addr)) {
        return invalid("observation cannot peek this memory-mapped device");
    }
    Ok(())
}
fn character(code: u8) -> char {
    if code < 128 {
        code as char
    } else {
        CP437_HIGH.chars().nth((code - 128) as usize).unwrap()
    }
}
const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ";
pub(super) fn text(machine: &Machine, p: &Value, revision: u64) -> Result<Value> {
    let (columns, display, mode) = text_layout(machine, p)?;
    let card = machine.bus().primary_video().unwrap();
    let active = (card.start_address() as usize * 2) & 0x3fff;
    let data = peek(machine, 0xb8000, 16384)?;
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    for y in 0..25 {
        let mut row = String::new();
        let mut cellrow = Vec::new();
        for x in 0..columns {
            let offset = (display + (y * columns + x) * 2) & 0x3fff;
            let code = data[offset];
            let attr = data[(offset + 1) & 0x3fff];
            row.push(if code < 32 { ' ' } else { character(code) });
            cellrow.push(json!({"code":code,"char":character(code).to_string(),"attribute":attr,
                "foreground":attr&15,"background":(attr>>4)&7,"blink":attr&128!=0}));
        }
        rows.push(row.trim_end().to_string());
        cells.push(cellrow);
    }
    let size = columns * 25 * 2;
    Ok(
        json!({"adapter":"CGA","columns":columns,"rows":25,"page_size_bytes":size,
        "page_count":16384/size,"page":display/size,"active_page":active/size,
        "is_active_page":display==active,"display_address":display,
        "mode":if columns==40 {"Text40"} else {"Text80"},"graphics_mode":mode,
        "text":rows,"cells":cells,"state_revision":revision}),
    )
}
pub(super) fn observe(a: &Agent, machine: &mut Machine, p: &Value) -> Result<Value> {
    if a.running {
        return invalid("target must be paused for this operation");
    }
    if number(&p["expected_state_revision"])? != a.revision {
        return invalid("expected state revision does not match the live state");
    }
    keys(
        p,
        &["expected_state_revision", "memory", "video_text", "video_memory"],
        "unsupported observation parameter",
    )?;
    let empty = Vec::new();
    let windows = match p.get("memory") {
        Some(v) => v.as_array().ok_or(("memory must contain at most 16 windows", -32602))?,
        None => &empty,
    };
    if windows.len() > 16 {
        return invalid("memory must contain at most 16 windows");
    }
    let mut ranges = Vec::new();
    let mut total = 0;
    for window in windows {
        keys(
            window,
            &["address", "offset", "length"],
            "memory window must contain only address and length",
        )?;
        let start = address(window.get("address").unwrap_or(&window["offset"]))?;
        let length = window.get("length").map(number).transpose()?.unwrap_or(1);
        if length == 0 || length > 65536 || start as u64 + length > 0x100000 {
            return invalid("length must be 1..65536 and stay within memory");
        }
        total += length;
        if total > 65536 {
            return invalid("observation memory exceeds 65536 bytes");
        }
        if (start..start + length as usize).any(|addr| !machine.bus().is_observable_memory(addr)) {
            return invalid("observation cannot peek this memory-mapped device");
        }
        ranges.push((start, length as usize));
    }
    if let Some(params) = p.get("video_text") {
        text_layout(machine, params)?;
    }
    let video = match p.get("video_memory") {
        Some(v) => v.as_bool().ok_or(("video_memory must be boolean", -32602))?,
        None => false,
    };
    if video {
        let card = machine.bus().primary_video().ok_or(("CGA adapter required", -32602))?;
        if card.video_type() != VideoType::CGA {
            return invalid("CGA adapter required");
        }
        video_mapping(machine)?;
    }
    let mut memory = Vec::new();
    for (start, length) in ranges {
        memory.push(bytes(&peek(machine, start, length)?, start, a.revision));
    }
    let mut result = json!({"state_revision":a.revision,"registers":registers(machine,a.revision),"memory":memory});
    if let Some(params) = p.get("video_text") {
        result["video_text"] = text(machine, params, a.revision)?;
    }
    if video {
        let mut descriptor = bytes(&peek(machine, 0xb8000, 16384)?, 0xb8000, a.revision);
        descriptor["adapter"] = json!("CGA");
        result["video_memory"] = descriptor;
    }
    Ok(result)
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
