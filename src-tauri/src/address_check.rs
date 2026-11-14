use futures_util::StreamExt;
use std::net::SocketAddr;

const ADDRESS_URL: &str = "http://www.yuanshen.com/query_server_address";
const MAX_BODY_BYTES: usize = 256;

fn parse_address(bytes: &[u8]) -> Result<String, String> {
  let text = std::str::from_utf8(bytes).map_err(|_| "地址接口返回的正文不是有效文本".to_string())?.trim();
  let address: SocketAddr = text.parse().map_err(|_| "地址接口正文必须是纯文本 IP:端口，不能是 JSON、HTML 或 XML".to_string())?;
  if address.port() == 0 || address.ip().is_unspecified() {
    return Err("地址接口返回了无效 IP 或端口，端口必须在 1–65535 之间".into());
  }
  Ok(address.to_string())
}

async fn read_endpoint(url: &str, proxy: Option<&str>, maximum: usize) -> Result<Vec<u8>, String> {
  let mut builder = reqwest::Client::builder().no_proxy()
    .connect_timeout(std::time::Duration::from_secs(3))
    .timeout(std::time::Duration::from_secs(8))
    .redirect(reqwest::redirect::Policy::none());
  if let Some(proxy) = proxy { builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?); }
  let client = builder.build().map_err(|e| e.to_string())?;
  let response = client.get(url).send().await.map_err(|e| format!("启动预检请求失败（{}）：{}", url, e))?;
  if response.status() != reqwest::StatusCode::OK {
    return Err(format!("接口 {} 返回 HTTP {}，期望 HTTP 200；请检查服务器入口和客户端版本", url, response.status().as_u16()));
  }
  let mut stream = response.bytes_stream();
  let mut body = Vec::new();
  while let Some(chunk) = stream.next().await {
    let chunk = chunk.map_err(|e| format!("读取接口响应失败：{}", e))?;
    if body.len() + chunk.len() > maximum { return Err("启动预检响应正文过长".into()); }
    body.extend_from_slice(&chunk);
  }
  Ok(body)
}

pub async fn via_proxy(proxy: &str) -> Result<String, String> {
  parse_address(&read_endpoint(ADDRESS_URL, Some(proxy), MAX_BODY_BYTES).await?)
}

pub async fn region_list(server: &str, proxy: Option<&str>) -> Result<(), String> {
  let server = crate::legacy34::server_address(server)?;
  let body = read_endpoint(&format!("{}/query_region_list", server), proxy, 128 * 1024).await?;
  validate_region_list(&body)
}

#[derive(Debug)]
enum ProtoValue<'a> { Number(u64), Bytes(&'a [u8]), Other }

fn varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
  let mut value = 0u64;
  for shift in (0..70).step_by(7) {
    let byte = *bytes.get(*cursor).ok_or("截断的分区列表数据")?;
    *cursor += 1;
    if shift == 63 && byte > 1 { return Err("无效的分区列表整数".into()); }
    value |= ((byte & 0x7f) as u64) << shift;
    if byte & 0x80 == 0 { return Ok(value); }
  }
  Err("无效的分区列表整数".into())
}

fn proto_fields(bytes: &[u8]) -> Result<Vec<(u32, ProtoValue<'_>)>, String> {
  let mut cursor = 0;
  let mut fields = Vec::new();
  while cursor < bytes.len() {
    let tag = varint(bytes, &mut cursor)?;
    if tag >> 3 == 0 || tag >> 3 > 0x1fffffff { return Err("无效的分区列表字段".into()); }
    let value = match tag & 7 {
      0 => ProtoValue::Number(varint(bytes, &mut cursor)?),
      2 => {
        let length = usize::try_from(varint(bytes, &mut cursor)?).map_err(|_| "分区列表字段过长")?;
        let end = cursor.checked_add(length).ok_or("分区列表字段过长")?;
        let value = bytes.get(cursor..end).ok_or("截断的分区列表字段")?;
        cursor = end;
        ProtoValue::Bytes(value)
      }
      1 | 5 => {
        cursor = cursor.checked_add(if tag & 7 == 1 { 8 } else { 4 }).ok_or("分区列表字段过长")?;
        if cursor > bytes.len() { return Err("截断的分区列表字段".into()); }
        ProtoValue::Other
      }
      _ => return Err("不支持的分区列表编码".into()),
    };
    fields.push(((tag >> 3) as u32, value));
  }
  Ok(fields)
}

fn validate_region_list(body: &[u8]) -> Result<(), String> {
  use base64::Engine;
  let text = std::str::from_utf8(body).map_err(|_| "3.4 分区接口返回了非文本正文")?;
  let compact: String = text.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
  let bytes = base64::engine::general_purpose::STANDARD.decode(compact)
    .map_err(|_| "3.4 分区接口必须返回 Base64 编码的分区列表，不能是 JSON、HTML 或 XML")?;
  let fields = proto_fields(&bytes)?;
  let mut count = 0;
  for (number, value) in fields {
    match (number, value) {
      (1, ProtoValue::Number(code)) if code != 0 => return Err(format!("3.4 分区接口返回错误码 {}", code)),
      (2, ProtoValue::Bytes(region)) => {
        let mut has_dispatch = false;
        for (field, value) in proto_fields(region)? {
          if let (4, ProtoValue::Bytes(url)) = (field, value) {
            let text = std::str::from_utf8(url).map_err(|_| "分区调度地址不是有效文本")?;
            let uri: tauri::http::Uri = text.parse().map_err(|_| "分区调度地址格式无效")?;
            if !matches!(uri.scheme_str(), Some("http") | Some("https")) || uri.host().is_none() {
              return Err("分区调度地址必须是有效的 HTTP(S) 地址".into());
            }
            has_dispatch = true;
          }
        }
        if !has_dispatch { return Err("3.4 分区列表缺少调度地址".into()); }
        count += 1;
      }
      _ => (),
    }
  }
  if count == 0 { return Err("3.4 分区接口未返回可用分区".into()); }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::{Read, Write};

  #[test]
  fn accepts_plain_socket_address_and_rejects_error_pages() {
    assert_eq!(parse_address(b"192.168.2.152:1983\n").unwrap(), "192.168.2.152:1983");
    for body in ["\"192.168.2.152:1983\"", "<Error>404</Error>", "<html>error</html>", "192.168.2.152:0", "192.168.2.152:65536", "0.0.0.0:1983", ""] {
      assert!(parse_address(body.as_bytes()).is_err(), "Accepted invalid address: {}", body);
    }
  }

  #[test]
  fn preflight_uses_actual_patch_url_through_explicit_proxy() {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    for (status, body, valid) in [("200 OK", "192.168.2.152:1983", true), ("404 Not Found", "<Error>404</Error>", false), ("200 OK", "\"192.168.2.152:1983\"", false)] {
      let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
      let proxy = format!("http://{}", listener.local_addr().unwrap());
      let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
          let count = stream.read(&mut buffer).unwrap();
          assert!(count > 0);
          request.extend_from_slice(&buffer[..count]);
        }
        assert!(String::from_utf8(request).unwrap().starts_with("GET http://www.yuanshen.com/query_server_address HTTP/1.1\r\n"));
        write!(stream, "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, body.len(), body).unwrap();
      });
      let result = runtime.block_on(via_proxy(&proxy));
      server.join().unwrap();
      assert_eq!(result.is_ok(), valid, "Unexpected preflight result: {:?}", result);
    }
  }
  #[test]
  fn recognizes_34_protobuf_region_list_and_rejects_47_or_error_bodies() {
    use base64::Engine;
    let url = b"http://192.168.2.152:20001/query_cur_region";
    let mut region = vec![0x22, url.len() as u8];
    region.extend_from_slice(url);
    let mut list = vec![0x12, region.len() as u8];
    list.extend_from_slice(&region);
    let valid = base64::engine::general_purpose::STANDARD.encode(&list);
    assert!(validate_region_list(valid.as_bytes()).is_ok());
    for invalid in [b"<Error>404</Error>".as_slice(), b"192.168.2.152:1983", b"{}", b"", b"EgE=", b"////"] {
      assert!(validate_region_list(invalid).is_err());
    }
    let failed = base64::engine::general_purpose::STANDARD.encode([0x08, 0x01]);
    assert!(validate_region_list(failed.as_bytes()).is_err());
  }

  #[test]
  fn legacy_preflight_uses_region_endpoint_through_explicit_proxy() {
    use base64::Engine;
    let url = b"http://192.168.2.152:20001/query_cur_region";
    let mut region = vec![0x22, url.len() as u8];
    region.extend_from_slice(url);
    let mut list = vec![0x12, region.len() as u8];
    list.extend_from_slice(&region);
    let body = base64::engine::general_purpose::STANDARD.encode(list);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
      let mut request = Vec::new();
      let mut buffer = [0; 1024];
      while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        request.extend_from_slice(&buffer[..count]);
      }
      assert!(String::from_utf8(request).unwrap().starts_with("GET http://192.168.2.152:2891/query_region_list HTTP/1.1\r\n"));
      write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    runtime.block_on(region_list("http://192.168.2.152:2891", Some(&proxy))).unwrap();
    server.join().unwrap();
  }

}
