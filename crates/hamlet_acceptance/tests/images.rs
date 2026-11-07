//! Images on a line: uploaded in parts, kept once by their hash,
//! carried by the line's transcript item and read back by id.

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::words;

/// A 1x1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

#[tokio::test]
async fn an_uploaded_image_goes_with_a_line_and_is_read_back_by_id() {
    let mut s =
        Session::start(Faux::new().responses([words("I see it.")]))
            .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    let png = BASE64_STANDARD.decode(PNG).unwrap();

    // Two parts, so the upload is whole only at the last.
    let kept = s.chief.put_image(&png, png.len() / 2 + 1).await;
    let image = kept.image.clone().expect("the last part names it");
    // The same bytes are kept once.
    let again = s.chief.put_image(&png, png.len()).await;
    assert_eq!(again.image.as_ref(), Some(&image));

    let refused = s.chief.put_image(b"not an image", 64).await;
    assert_eq!(refused.kind, ReplyKind::Error);

    let said = s
        .chief
        .say_with(hand, "Look at this.", vec![image.clone()])
        .await;
    assert_eq!(said.kind, ReplyKind::Notice);
    s.chief.shows(&now, "I see it.").await;
    let items = s.chief.items(&now);
    assert_eq!(items[0].text, "Look at this.");
    assert_eq!(items[0].images, [image.clone()]);

    let ServerMessage::Read(ReadResult::Image {
        image: bytes, ..
    }) = s
        .chief
        .read(|id| Read::Image {
            id,
            image: image.clone(),
            size: None,
        })
        .await
    else {
        panic!("expected the image");
    };
    assert_eq!(bytes.mime, "image/png");
    assert_eq!(BASE64_STANDARD.decode(bytes.data).unwrap(), png);
}
