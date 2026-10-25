//! WiFi setup hotspot: an open access point serving a one-page form where you
//! enter your network's name and password.
//!
//! Three tiny servers run on the hotspot's network stack:
//! - DHCP (UDP 67): hands phones/laptops an address so they can join at all.
//! - DNS (UDP 53): answers every lookup with our address, so the OS's
//!   captive-portal check lands on the form and pops it up automatically.
//! - HTTP (TCP 80): the form itself.

use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Stack};
use embassy_time::Duration;
use embedded_io_async::Write;
use log::{info, warn};

use crate::config::DEVICE_NAME;
use crate::settings::{MAX_PASSWORD_LEN, MAX_SSID_LEN};
use crate::{SharedSettings, restart};

pub const AP_ADDRESS: Ipv4Address = Ipv4Address::new(192, 168, 4, 1);
const LEASE_BASE: u8 = 100; // clients get 192.168.4.100..
const MAX_LEASES: usize = 16;

// ------------------------------------------------------------------- HTTP

#[embassy_executor::task]
pub async fn http_task(stack: Stack<'static>, settings: SharedSettings) {
    let mut rx_buf = [0u8; 1536];
    let mut tx_buf = [0u8; 2048];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(10)));
        if socket.accept(80).await.is_err() {
            continue;
        }
        let saved = handle_http(&mut socket, settings).await;
        socket.close();
        let _ = socket.flush().await;
        if saved {
            info!("WiFi credentials saved, restarting");
            restart().await;
        }
        socket.abort();
    }
}

/// Serve one request. Returns true once new credentials were saved.
async fn handle_http(socket: &mut TcpSocket<'_>, settings: SharedSettings) -> bool {
    let mut buf = [0u8; 1024];
    let mut len = 0;
    // Read until the end of the headers, then the body by Content-Length.
    let header_end = loop {
        if len == buf.len() {
            return false;
        }
        match socket.read(&mut buf[len..]).await {
            Ok(0) | Err(_) => return false,
            Ok(n) => len += n,
        }
        if let Some(i) = find(&buf[..len], b"\r\n\r\n") {
            break i + 4;
        }
    };
    let (is_save, content_length) = {
        let head = core::str::from_utf8(&buf[..header_end]).unwrap_or("");
        let content_length = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        (head.starts_with("POST /save"), content_length)
    };
    while len < (header_end + content_length).min(buf.len()) {
        match socket.read(&mut buf[len..]).await {
            Ok(0) | Err(_) => break,
            Ok(n) => len += n,
        }
    }

    if !is_save {
        let _ = respond(socket, "200 OK", FORM_PAGE).await;
        return false;
    }

    let body = &buf[header_end..len];
    let mut ssid = [0u8; MAX_SSID_LEN];
    let mut password = [0u8; MAX_PASSWORD_LEN];
    let ssid = form_field(body, b"ssid", &mut ssid);
    let password = form_field(body, b"password", &mut password);
    let (Some(ssid), Some(password)) = (ssid, password) else {
        let _ = respond(socket, "400 Bad Request", INVALID_PAGE).await;
        return false;
    };
    if ssid.is_empty() || (!password.is_empty() && password.len() < 8) {
        let _ = respond(socket, "400 Bad Request", INVALID_PAGE).await;
        return false;
    }
    match settings.lock().await.set_wifi(ssid, password) {
        Ok(()) => {
            let _ = respond(socket, "200 OK", SAVED_PAGE).await;
            true
        }
        Err(e) => {
            warn!("saving WiFi credentials failed: {e:?}");
            let _ = respond(socket, "500 Internal Server Error", ERROR_PAGE).await;
            false
        }
    }
}

async fn respond(socket: &mut TcpSocket<'_>, status: &str, body: &str) -> Result<(), ()> {
    for part in [
        "HTTP/1.1 ",
        status,
        "\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        PAGE_HEAD,
        body,
        PAGE_TAIL,
    ] {
        socket.write_all(part.as_bytes()).await.map_err(|_| ())?;
    }
    socket.flush().await.map_err(|_| ())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Extract and URL-decode one field of an `application/x-www-form-urlencoded` body.
fn form_field<'a>(body: &[u8], name: &[u8], out: &'a mut [u8]) -> Option<&'a str> {
    let value = body.split(|&b| b == b'&').find_map(|pair| {
        let eq = pair.iter().position(|&b| b == b'=')?;
        (&pair[..eq] == name).then(|| &pair[eq + 1..])
    })?;
    let mut n = 0;
    let mut i = 0;
    while i < value.len() {
        let byte = match value[i] {
            b'+' => b' ',
            b'%' if i + 2 < value.len() => {
                let hex = core::str::from_utf8(&value[i + 1..i + 3]).ok()?;
                i += 2;
                u8::from_str_radix(hex, 16).ok()?
            }
            b'%' => return None,
            b => b,
        };
        *out.get_mut(n)? = byte;
        n += 1;
        i += 1;
    }
    core::str::from_utf8(&out[..n]).ok()
}

const PAGE_HEAD: &str = r#"<!doctype html><html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>nxcontroller setup</title>
<style>body{font-family:system-ui,sans-serif;max-width:420px;margin:2em auto;padding:0 16px;line-height:1.45}
label{display:block;font-weight:600}input,button{width:100%;padding:.65em;margin:.35em 0 1.1em;font-size:1em;box-sizing:border-box;border:1px solid #bbb;border-radius:6px}
button{background:#1a1a1a;color:#fff;border:0;cursor:pointer}small{color:#666}</style></head><body><h1>nxcontroller setup</h1>"#;
const PAGE_TAIL: &str = "</body></html>";

const FORM_PAGE: &str = r#"<p>Connect this board to your WiFi. 2.4&nbsp;GHz networks only.</p>
<form method="post" action="/save">
<label>Network name<input name="ssid" maxlength="32" required autocapitalize="none" autocorrect="off" spellcheck="false"></label>
<label>Password<input name="password" type="password" maxlength="64" autocomplete="off"></label>
<button>Save and connect</button></form>
<p><small>Leave the password empty for an open network. The board restarts and joins your network. Then run <code>nxc discover</code> on a computer on that network.</small></p>"#;

const SAVED_PAGE: &str = r#"<p><b>Saved.</b> The board is restarting and joining your network. This hotspot disappears in a moment.</p>
<p><small>If the hotspot comes back after about 30 seconds, the board couldn't connect: check the name and password and try again.</small></p>"#;

const INVALID_PAGE: &str = r#"<p><b>That didn't look right.</b> The network name needs 1 to 32 characters, and a password needs 8 to 64 (or leave it empty for an open network).</p>
<p><a href="/">Try again</a></p>"#;

const ERROR_PAGE: &str =
    r#"<p><b>Couldn't save the settings on the board.</b></p><p><a href="/">Try again</a></p>"#;

// ------------------------------------------------------------------- DHCP

#[embassy_executor::task]
pub async fn dhcp_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1200];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1200];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if let Err(e) = socket.bind(67) {
        warn!("DHCP bind failed: {e:?}");
        return;
    }

    let mut leases = [[0u8; 6]; MAX_LEASES];
    let mut next_lease = 0;
    let mut buf = [0u8; 600];
    let mut reply = [0u8; 300];
    let broadcast = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)), 68);

    loop {
        let Ok((n, _)) = socket.recv_from(&mut buf).await else {
            continue;
        };
        let request = &buf[..n];
        // BOOTREQUEST with the DHCP magic cookie.
        if n < 240 || request[0] != 1 || request[236..240] != [99, 130, 83, 99] {
            continue;
        }
        let reply_type = match dhcp_message_type(&request[240..]) {
            Some(1) => 2, // DISCOVER -> OFFER
            Some(3) => 5, // REQUEST -> ACK
            _ => continue,
        };
        let mac: [u8; 6] = request[28..34].try_into().unwrap();
        let slot = match leases.iter().position(|m| *m == mac) {
            Some(slot) => slot,
            None => {
                let slot = next_lease;
                leases[slot] = mac;
                next_lease = (next_lease + 1) % MAX_LEASES;
                slot
            }
        };
        let client = [192, 168, 4, LEASE_BASE + slot as u8];
        let server = AP_ADDRESS.octets();

        reply.fill(0);
        reply[0] = 2; // BOOTREPLY
        reply[1] = 1; // Ethernet
        reply[2] = 6; // MAC length
        reply[4..8].copy_from_slice(&request[4..8]); // xid
        reply[10..12].copy_from_slice(&request[10..12]); // flags
        reply[16..20].copy_from_slice(&client); // yiaddr
        reply[20..24].copy_from_slice(&server); // siaddr
        reply[28..44].copy_from_slice(&request[28..44]); // chaddr
        reply[236..240].copy_from_slice(&[99, 130, 83, 99]);
        let mut o = 240;
        let mut option = |code: u8, data: &[u8]| {
            reply[o] = code;
            reply[o + 1] = data.len() as u8;
            reply[o + 2..o + 2 + data.len()].copy_from_slice(data);
            o += 2 + data.len();
        };
        option(53, &[reply_type]);
        option(54, &server); // server identifier
        option(51, &3600u32.to_be_bytes()); // lease time
        option(1, &[255, 255, 255, 0]); // subnet mask
        option(3, &server); // router
        option(6, &server); // DNS
        reply[o] = 255; // end
        let len = o + 1;

        if let Err(e) = socket.send_to(&reply[..len], broadcast).await {
            warn!("DHCP reply failed: {e:?}");
        }
    }
}

fn dhcp_message_type(mut options: &[u8]) -> Option<u8> {
    while let [code, rest @ ..] = options {
        match *code {
            0 => options = rest,
            255 => return None,
            _ => {
                let len = *rest.first()? as usize;
                let data = rest.get(1..1 + len)?;
                if *code == 53 {
                    return data.first().copied();
                }
                options = &rest[1 + len..];
            }
        }
    }
    None
}

// -------------------------------------------------------------------- DNS

#[embassy_executor::task]
pub async fn dns_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1024];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if let Err(e) = socket.bind(53) {
        warn!("DNS bind failed: {e:?}");
        return;
    }

    let mut buf = [0u8; 512];
    loop {
        let Ok((n, meta)) = socket.recv_from(&mut buf).await else {
            continue;
        };
        // Header (12) + at least a root name, type and class. One question only.
        if n < 17 || buf[2] & 0x80 != 0 || buf[4..6] != [0, 1] {
            continue;
        }
        // Walk the question name to find its end.
        let mut i = 12;
        while i < n && buf[i] != 0 {
            i += 1 + buf[i] as usize;
        }
        let question_end = i + 5; // zero byte + type + class
        if question_end > n || question_end + 16 > buf.len() {
            continue;
        }
        let is_a_record = buf[i + 1..i + 5] == [0, 1, 0, 1];

        buf[2] = 0x81; // response, recursion desired
        buf[3] = 0x80; // recursion available, no error
        buf[6..12].copy_from_slice(&[0, is_a_record as u8, 0, 0, 0, 0]);
        let mut len = question_end;
        if is_a_record {
            let answer = [
                0xC0, 0x0C, // pointer to the question name
                0, 1, 0, 1, // type A, class IN
                0, 0, 0, 60, // TTL
                0, 4, // address length
            ];
            buf[len..len + answer.len()].copy_from_slice(&answer);
            len += answer.len();
            buf[len..len + 4].copy_from_slice(&AP_ADDRESS.octets());
            len += 4;
        }
        let _ = socket.send_to(&buf[..len], meta.endpoint).await;
    }
}

/// Log line for the serial console when the hotspot comes up.
pub fn announce() {
    info!(
        "Setup hotspot '{}' is up: join it and open http://{} to enter WiFi details for '{}'",
        crate::config::SETUP_SSID,
        AP_ADDRESS,
        DEVICE_NAME
    );
}
