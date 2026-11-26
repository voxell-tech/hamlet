//! Images and files a chief uploads and sends with a line.

use base64::Engine as _;
use hamlet_acceptance::Client;
use hamlet_protocol::client::{
    AgentId, AttachedImage, ImageSize, Read, ReadResult, ReplyKind,
    ServerMessage,
};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::commands::found;
use super::ended::{ended, hire_blank};
use super::{look, serve};

fn png(side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let pixels = image::RgbImage::from_fn(side, side, |x, y| {
        image::Rgb([(x * 7 % 256) as u8, (y * 13 % 256) as u8, 90])
    });
    PngEncoder::new(&mut out)
        .write_image(
            pixels.as_raw(),
            side,
            side,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    out
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upload_is_read_back_as_it_was_sent() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let bytes = png(600);
    let kept = chief.put_image(&bytes, 4096).await;
    let image =
        kept.image.clone().unwrap_or_else(|| panic!("{kept:?}"));
    let read = |size| {
        let image = image.clone();
        move |id| Read::Image { id, image, size }
    };
    let original = chief.read(read(None)).await;
    let thumbnail =
        chief.read(read(Some(ImageSize::Thumbnail))).await;
    let file = chief.put_file(b"Some notes.\n", 5).await;

    assert_eq!(
        (kept.kind, kept.text.as_str()),
        (ReplyKind::Notice, "Kept the image.")
    );
    assert_eq!(image.to_string(), sha256(&bytes));
    let ServerMessage::Read(ReadResult::Image {
        image: read, ..
    }) = original
    else {
        panic!("an image read gives the image: {original:?}");
    };
    assert_eq!(read.mime, "image/png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&read.data)
            .unwrap(),
        bytes
    );
    let ServerMessage::Read(ReadResult::Image {
        image: small, ..
    }) = thumbnail
    else {
        panic!("a thumbnail read gives an image: {thumbnail:?}");
    };
    let side = |data: &str| {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        image::load_from_memory(&bytes).unwrap().width()
    };
    assert_eq!((side(&read.data), side(&small.data)), (600, 320));
    assert_eq!(
        (file.kind, file.text.as_str()),
        (ReplyKind::Notice, "Kept the file.")
    );
    assert_eq!(
        file.file.map(|file| file.to_string()),
        Some(sha256(b"Some notes.\n"))
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upload_out_of_its_bounds_is_refused_in_todays_words() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let empty = chief.put_image(b"", 10).await;
    let not_one = chief.put_image(b"not an image at all", 7).await;
    let large = chief.put_image(&vec![0; 21 << 20], 1 << 20).await;
    let missing = chief
        .read(|id| Read::Image {
            id,
            image: "b".repeat(64).parse().unwrap(),
            size: None,
        })
        .await;

    let words = |reply: hamlet_protocol::client::Reply| {
        assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
        reply.text
    };
    assert_eq!(words(empty), "An image has bytes.");
    assert_eq!(
        words(not_one),
        "An image is a PNG, JPEG, WebP or GIF"
    );
    assert_eq!(words(large), "An image is at most 20 MiB.");
    let ServerMessage::Reply(missing) = missing else {
        panic!("a missing image is refused: {missing:?}");
    };
    assert_eq!(
        missing.text,
        format!("no image `{}` was sent first", "b".repeat(64))
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_with_images_to_a_model_that_takes_none_is_refused() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    chief.command(mayor, "/model mayor faux::unlisted").await;
    let image = AttachedImage::last(sha256(&png(8)).parse().unwrap());
    let refused = chief.say_with(mayor, "Look.", vec![image]).await;

    assert_eq!(
        (refused.kind, refused.text.as_str()),
        (
            ReplyKind::Error,
            "the model `faux::unlisted` takes no images; choose \
             another model or leave the images out"
        )
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_with_an_image_to_an_agent_not_there_is_answered_as_one_without()
 {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    let docs = look(&server).await.hamlets[0].id.clone();
    let address = hire_blank(&mut chief, &docs).await;
    chief.command(mayor, &format!("/end {address}")).await;
    ended(&server, &address).await;
    let realm = look(&server).await;
    let hire = realm
        .agents
        .iter()
        .find(|agent| agent.name == address)
        .unwrap()
        .id;
    let image = AttachedImage::last(sha256(&png(8)).parse().unwrap());
    for to in [hire, AgentId(99)] {
        let with =
            chief.say_with(to, "Look.", vec![image.clone()]).await;
        let without = chief.say_with(to, "Look.", Vec::new()).await;

        assert_eq!(with.kind, ReplyKind::Error, "{}", with.text);
        assert_eq!(with.text, without.text);
    }
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
