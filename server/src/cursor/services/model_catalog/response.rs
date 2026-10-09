use super::*;

pub(crate) fn merge_response(
    upstream: proxy::BufferedResponse,
    extra: Vec<u8>,
    hide_builtin: bool,
) -> Result<Response<Body>> {
    let (framed, payload) = unary_payload(&upstream.body).unwrap_or((false, &[]));
    if !upstream.status.is_success() || hide_builtin {
        if hide_builtin {
            tracing::info!(
                "built-in Cursor models hidden: served local Nexusor model catalog to Cursor"
            );
        } else if upstream.status == axum::http::StatusCode::UNAUTHORIZED {
            tracing::info!(
                "Cursor cloud upstream unauthenticated (401); using local Nexusor model catalog"
            );
        } else {
            tracing::warn!(status = %upstream.status, "Cursor model catalog upstream rejected request; using local catalog");
        }
        let body = if framed {
            let mut out = BytesMut::with_capacity(5 + extra.len());
            out.put_u8(0);
            out.put_u32(extra.len() as u32);
            out.extend_from_slice(&extra);
            out.freeze()
        } else {
            Bytes::from(extra)
        };
        return Ok(upstream.with_body(body));
    }
    let body = if framed {
        let mut merged = BytesMut::with_capacity(5 + payload.len() + extra.len());
        merged.put_u8(0);
        merged.put_u32((payload.len() + extra.len()) as u32);
        merged.extend_from_slice(payload);
        merged.extend_from_slice(&extra);
        merged.freeze()
    } else {
        let mut merged = BytesMut::with_capacity(payload.len() + extra.len());
        merged.extend_from_slice(payload);
        merged.extend_from_slice(&extra);
        merged.freeze()
    };
    Ok(upstream.with_body(body))
}

pub(crate) fn local_response(body: Vec<u8>) -> Response<Body> {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    response
}

pub(crate) fn unary_payload(body: &Bytes) -> Result<(bool, &[u8])> {
    if body.len() < 5 {
        return Ok((false, body));
    }
    let flags = body[0];
    let length = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if length != body.len() - 5 {
        return Ok((false, body));
    }
    if flags != 0 {
        return Err(Error::Protocol(format!(
            "cannot merge compressed or terminal model catalog frame: flags={flags}"
        )));
    }
    Ok((true, &body[5..]))
}
