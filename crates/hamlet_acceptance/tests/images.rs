//! Images on a line: uploaded in parts, kept once by their hash,
//! carried by the line's transcript item and read back by id, and
//! given to a model its catalogue card says takes them.

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{openai, words};

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
        .say_with(
            hand,
            "Look at this.",
            vec![AttachedImage::last(image.clone())],
        )
        .await;
    assert_eq!(said.kind, ReplyKind::Notice);
    s.chief.shows(&now, "I see it.").await;
    let items = s.chief.items(&now);
    assert_eq!(items[0].text, "Look at this.");
    assert_eq!(items[0].images, [AttachedImage::last(image.clone())]);

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

#[tokio::test]
async fn a_model_the_chief_says_takes_images_is_given_them() {
    let mut endpoint = openai::Endpoint::start().await;
    let mut s = Session::with_endpoint(&endpoint).await;
    // A model is carded once the server has its listing.
    let set = EditCatalogueSet {
        images: Some(Some(ImageInput {
            per_line: Some(2),
            ..ImageInput::default()
        })),
        note: None,
        price: None,
    };
    let mut carded = None;
    for _ in 0..100 {
        let reply = s
            .chief
            .request(|id| {
                ClientMessage::Edit(Edit::Catalogue {
                    id,
                    model: "local::org/large".into(),
                    set: set.clone(),
                })
            })
            .await;
        if reply.kind == ReplyKind::Notice {
            carded = Some(reply);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100))
            .await;
    }
    assert!(carded.is_some(), "the model is never listed");
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    let png = BASE64_STANDARD.decode(PNG).unwrap();
    let image =
        s.chief.put_image(&png, png.len()).await.image.unwrap();

    let three = vec![AttachedImage::last(image.clone()); 3];
    let refused = s.chief.say_with(hand, "Look.", three).await;
    assert_eq!(refused.kind, ReplyKind::Error);
    assert_eq!(
        refused.text,
        "the model `local::org/large` takes at most 2 images in a \
         line"
    );

    let said = s
        .chief
        .say_with(
            hand,
            "Look at this.",
            vec![AttachedImage::last(image)],
        )
        .await;
    assert_eq!(said.kind, ReplyKind::Notice, "{said:?}");
    s.chief.shows(&now, "Hello.").await;
    let asked = endpoint
        .requests()
        .iter()
        .filter_map(|r| r.body.clone())
        .find(|body| body["messages"].is_array())
        .expect("the model is asked");
    let parts = asked["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .map(|part| part["type"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(parts.contains(&"image_url"), "{asked}");
}
