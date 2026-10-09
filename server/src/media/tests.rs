use super::*;

#[test]
fn read_gate_preserves_bounded_media_but_limits_other_binary() {
    use crate::{
        cursor::{protocol::proto::agent::v1 as pb, tools::tool_call_result::ToolCompletion},
        model::{ToolCall, ToolResult},
    };
    for (path, data, preserved) in [
        ("clip.mp4", vec![0; 40 * 1024], true),
        (
            "document.pdf",
            [b"%PDF-".as_slice(), &vec![0; 40 * 1024]].concat(),
            true,
        ),
        ("data.bin", vec![0; 40 * 1024], false),
        (
            "oversized.pdf",
            [b"%PDF-".as_slice(), &vec![0; 32 * 1024 * 1024]].concat(),
            false,
        ),
    ] {
        let call = ToolCall {
            index: 0,
            call_id: "read".into(),
            model_call_id: "model".into(),
            name: "Read".into(),
            arguments_text: "{}".into(),
            arguments: serde_json::json!({}),
            argument_error: None,
        };
        let completion = ToolCompletion::new(
            &call,
            0,
            ToolResult {
                call_id: call.call_id.clone(),
                content: "binary".into(),
                is_error: false,
                image: None,
                images: vec![],
            },
            pb::tool_call::Tool::ReadToolCall(pb::ReadToolCall {
                result: Some(pb::ReadToolResult {
                    result: Some(pb::read_tool_result::Result::Success(pb::ReadToolSuccess {
                        path: path.into(),
                        output: Some(pb::read_tool_success::Output::Data(data)),
                        ..Default::default()
                    })),
                }),
                ..Default::default()
            }),
        );
        let Some(pb::tool_call::Tool::ReadToolCall(tool)) = completion.tool_call().tool.as_ref()
        else {
            panic!("Read expected")
        };
        let Some(pb::read_tool_result::Result::Success(success)) =
            tool.result.as_ref().and_then(|r| r.result.as_ref())
        else {
            panic!("success expected")
        };
        assert_eq!(
            matches!(success.output, Some(pb::read_tool_success::Output::Data(_))),
            preserved,
            "{path}"
        );
        assert_eq!(success.exceeded_limit, !preserved, "{path}");
    }
}

#[tokio::test]
#[ignore = "requires local speech runtime and NEXUSOR_MEDIA_TEST_VIDEO English speech fixture"]
async fn read_video_preserves_images_payload_and_real_speech() {
    use crate::{
        cursor::{protocol::proto::agent::v1 as pb, tools::tool_call_result::ToolCompletion},
        model::{ToolCall, ToolResult},
    };
    let path = std::env::var("NEXUSOR_MEDIA_TEST_VIDEO").expect("set path to audio-smoke.mp4");
    let data = std::fs::read(&path).unwrap();
    let call = ToolCall {
        index: 0,
        call_id: "video-read".into(),
        model_call_id: "video-model".into(),
        name: "Read".into(),
        arguments_text: serde_json::json!({"path": path}).to_string(),
        arguments: serde_json::json!({"path": path}),
        argument_error: None,
    };
    let mut completion = ToolCompletion::new(
        &call,
        0,
        ToolResult {
            call_id: call.call_id.clone(),
            content: "Video binary".into(),
            is_error: false,
            image: None,
            images: vec![],
        },
        pb::tool_call::Tool::ReadToolCall(pb::ReadToolCall {
            result: Some(pb::ReadToolResult {
                result: Some(pb::read_tool_result::Result::Success(pb::ReadToolSuccess {
                    path,
                    output: Some(pb::read_tool_success::Output::Data(data.clone())),
                    ..Default::default()
                })),
            }),
            ..Default::default()
        }),
    );
    completion.render_read_media().await;
    let images = completion.take_mcp_images();
    assert!(
        !images.is_empty() && images.len() <= 12,
        "{}",
        completion.result().content
    );
    for image in images {
        completion.persist_mcp_image(&crate::store::BlobId::digest(&image.data), &image);
    }
    let text = &completion.result().content;
    assert!(text.contains("Local automatic speech transcript"), "{text}");
    assert!(
        text.contains("blue lantern") && text.contains("Tuesday"),
        "{text}"
    );
    assert!(
        text.contains("first audio stream") && text.contains("not verified video synchronization")
    );
    assert!(!completion.result().images.is_empty());
    assert!(
        matches!(completion.tool_call().tool.as_ref(), Some(pb::tool_call::Tool::ReadToolCall(tool)) if matches!(tool.result.as_ref().and_then(|r| r.result.as_ref()), Some(pb::read_tool_result::Result::Success(success)) if matches!(&success.output, Some(pb::read_tool_success::Output::Data(original)) if original == &data)))
    );
}

#[tokio::test]
async fn rejects_invalid_media_before_starting_decoder() {
    assert!(render(&[], "pdf").await.is_err());
    assert!(render(b"data", "unknown").await.is_err());
    assert!(render(&vec![0; 32 * 1024 * 1024 + 1], "pdf").await.is_err());
    assert_eq!(kind(b"%PDF-1.5", "", ""), Some("pdf"));
    assert_eq!(kind(b"data", "video/mp4", ""), Some("video"));
    assert_eq!(kind(b"data", "", "clip.MP4"), Some("video"));
    assert_eq!(kind(b"text", "text/plain", "notes.txt"), None);
}

#[tokio::test]
#[ignore = "requires scripts/setup-media.ps1 local runtime"]
async fn local_pdf_renderer_preserves_page_order_and_rejects_broken_input() {
    let data = crate::documents::tests::compressed_pdf();
    let media = render(&data, "pdf").await.unwrap();
    assert_eq!(media.images.len(), 2);
    assert_eq!(media.images[0].0, "PDF page 1");
    assert_eq!(media.images[1].0, "PDF page 2");
    assert!(media.summary.contains("0 pages omitted"));
    assert!(media
        .images
        .iter()
        .all(|(_, bytes)| bytes.starts_with(b"\x89PNG")));
    assert!(render(b"%PDF-broken", "pdf").await.is_err());
}

#[tokio::test]
#[ignore = "requires scripts/setup-media.ps1 local runtime"]
async fn read_pdf_images_keep_page_labels_and_original_read_payload() {
    use crate::{
        cursor::{protocol::proto::agent::v1 as pb, tools::tool_call_result::ToolCompletion},
        model::{ToolCall, ToolResult},
    };
    let call = ToolCall {
        index: 0,
        call_id: "pdf-read".into(),
        model_call_id: "pdf-model".into(),
        name: "Read".into(),
        arguments_text: r#"{"path":"report.pdf"}"#.into(),
        arguments: serde_json::json!({"path":"report.pdf"}),
        argument_error: None,
    };
    let mut completion = ToolCompletion::new(
        &call,
        0,
        ToolResult {
            call_id: call.call_id.clone(),
            content: "PDF text layer".into(),
            is_error: false,
            image: None,
            images: vec![],
        },
        pb::tool_call::Tool::ReadToolCall(pb::ReadToolCall {
            result: Some(pb::ReadToolResult {
                result: Some(pb::read_tool_result::Result::Success(pb::ReadToolSuccess {
                    path: "report.pdf".into(),
                    output: Some(pb::read_tool_success::Output::Data(
                        crate::documents::tests::compressed_pdf(),
                    )),
                    ..Default::default()
                })),
            }),
            ..Default::default()
        }),
    );
    completion.render_read_media().await;
    let images = completion.take_mcp_images();
    assert_eq!(images.len(), 2);
    for image in images {
        let id = crate::store::BlobId::digest(&image.data);
        completion.persist_mcp_image(&id, &image);
    }
    assert_eq!(completion.result().images.len(), 2);
    assert!(completion
        .result()
        .content
        .contains("Image 2: report.pdf: PDF page 2"));
    assert!(
        matches!(completion.tool_call().tool.as_ref(), Some(pb::tool_call::Tool::ReadToolCall(tool)) if matches!(tool.result.as_ref().and_then(|r| r.result.as_ref()), Some(pb::read_tool_result::Result::Success(success)) if matches!(&success.output, Some(pb::read_tool_success::Output::Data(data)) if data.starts_with(b"%PDF-"))))
    );
}
