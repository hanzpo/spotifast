//! Someone's profile: who they are and the playlists they show there.

use crate::api::models::UserProfile;
use crate::app::App;
use crate::i18n::{gettext, ngettext};
use crate::model::{Loadable, Page};
use crate::theme;
use crate::util;

use super::collection::{Hero, hero, hero_images};
use super::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui, id: &str) {
    if !app.user_pages.contains_key(id) {
        app.ensure_loaded(Page::User(id.to_string()));
    }
    let Some(page) = app.user_pages.remove(id) else {
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    match &page.profile {
        Loadable::Loaded(profile) => {
            profile_hero(app, ui, profile);
            theme::section_title(ui, &palette, &gettext(locale, "Public playlists"));
            ui.add_space(6.0);
            if profile.playlists.is_empty() {
                theme::subtle(ui, &palette, &gettext(locale, "No public playlists."));
            } else {
                let height = widgets::card_row_height(ui);
                widgets::virtual_wrapped_cards(ui, profile.playlists.len(), height, |ui, index| {
                    super::search::playlist_card(app, ui, &profile.playlists[index]);
                });
            }
        }
        Loadable::Loading | Loadable::NotLoaded => {
            loading_hero(app, ui, id);
            widgets::loading_row(ui, &palette, locale);
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            loading_hero(app, ui, id);
            widgets::error_row(ui, app, &error, Some(Page::User(id.to_string())));
        }
    }
    app.user_pages.insert(id.to_string(), page);
}

fn profile_hero(app: &mut App, ui: &mut egui::Ui, profile: &UserProfile) {
    let locale = app.locale;
    let mut byline = Vec::new();
    if !profile.playlists.is_empty() {
        let count = u32::try_from(profile.playlists.len()).unwrap_or(u32::MAX);
        byline.push((
            ngettext(
                locale,
                "{count} public playlist",
                "{count} public playlists",
                count,
            )
            .replace("{count}", &util::format_count(u64::from(count))),
            None,
        ));
    }
    if let Some(followers) = profile.followers {
        byline.push((
            ngettext(
                locale,
                "{count} follower",
                "{count} followers",
                u32::try_from(followers).unwrap_or(u32::MAX),
            )
            .replace("{count}", &util::format_count(followers)),
            None,
        ));
    }
    hero(
        app,
        ui,
        Hero {
            images: hero_images(&profile.images, None, false),
            liked: false,
            kind: gettext(locale, "Profile"),
            title: &profile.name,
            description: None,
            byline,
            round: true,
        },
    );
}

/// The name already known for the id, while the profile is on its way.
fn loading_hero(app: &mut App, ui: &mut egui::Ui, id: &str) {
    let name = app
        .user_names
        .get(id)
        .cloned()
        .flatten()
        .unwrap_or_else(|| id.to_string());
    let kind = gettext(app.locale, "Profile");
    hero(
        app,
        ui,
        Hero {
            images: Default::default(),
            liked: false,
            kind,
            title: &name,
            description: None,
            byline: Vec::new(),
            round: true,
        },
    );
}
