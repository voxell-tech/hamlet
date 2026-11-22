//! The browser the server's sign-in addresses open in, and which
//! addresses it takes.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;

pub fn plugin(app: &mut App) {
    app.init_resource::<Browser>();
}

/// The browser used for the server's login URLs.
#[derive(Resource)]
pub struct Browser(pub fn(&mut World, &str));

impl Default for Browser {
    fn default() -> Self {
        Self(|_, url| {
            hamlet_desktop::open::browse(url);
        })
    }
}

/// Whether `text` is an address the browser opens: https, or http to
/// the machine itself.
pub fn is_browsable(text: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(text) else {
        return false;
    };
    match url.scheme() {
        "https" => true,
        "http" => url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }),
        _ => false,
    }
}
