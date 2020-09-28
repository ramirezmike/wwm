use super::{
    component::Component, component::ComponentText, get_windows, item::Item,
    item_section::ItemSection, with_bar_by, Bar, BARS,
};
use crate::{
    config::Config, display::Display, event::Event, keybindings::KbManager, system::Rectangle,
    system::WindowId, tile_grid::TileGrid, window::Api, window::WindowEvent, CHANNEL, CONFIG,
    DISPLAYS, GRIDS, WORKSPACE_ID,
};
use log::{debug, error, info};
use parking_lot::Mutex;
use std::{sync::Arc, thread, time::Duration};

fn spawn_refresh_thread() {
    thread::spawn(|| loop {
        thread::sleep(Duration::from_millis(200));

        if get_windows().is_empty() {
            break;
        }

        CHANNEL
            .sender
            .clone()
            .send(Event::RedrawAppBar)
            .expect("Failed to send redraw-app-bar event");
    });
}

fn draw_component_text(
    api: &Api,
    rect: &Rectangle,
    config: &Config,
    component_text: &ComponentText,
) {
    let text = component_text.get_text();

    if text.is_empty() {
        return;
    }

    let fg = component_text.get_fg().unwrap_or(if config.light_theme {
        0x00333333
    } else {
        0x00ffffff
    });

    let bg = component_text.get_bg().unwrap_or(config.bar.color as u32);

    api.set_text_color(fg);
    api.set_background_color(bg);
    api.write_text(&text, rect.left, rect.top, true, false)
}

fn draw_components(
    api: &Api,
    display: &Display,
    kb_manager: &KbManager,
    grids: &Vec<TileGrid>,
    config: &Config,
    workspace_id: i32,
    height: i32,
    mut offset: i32,
    components: &[Component],
) {
    for component in components {
        let component_texts = component.render(kb_manager, display, grids, workspace_id, config);

        for (_i, component_text) in component_texts.iter().enumerate() {
            let width = api.calculate_text_rect(&component_text.get_text()).width();

            let rect = Rectangle {
                left: offset,
                right: offset + width,
                bottom: height,
                top: 0,
            };

            offset = rect.right;

            draw_component_text(api, &rect, config, &component_text);
        }
    }
}

fn components_to_section(
    api: &Api,
    display: &Display,
    kb_manager: &KbManager,
    grids: &Vec<TileGrid>,
    config: &Config,
    workspace_id: i32,
    components: &[Component],
) -> ItemSection {
    let mut section = ItemSection::default();
    let mut component_offset = 0;

    for component in components {
        let mut item = Item::default();
        let mut component_text_offset = 0;
        let mut component_width = 0;

        for component_text in component.render(kb_manager, display, grids, workspace_id, config) {
            let width = api.calculate_text_rect(&component_text.get_text()).width();
            let left = component_text_offset;
            let right = component_text_offset + width;

            item.widths.push((left, right));

            component_width += width;
            component_text_offset += width;
        }

        item.left = component_offset;
        item.right = item.left + component_width;
        item.component = component.clone();

        section.items.push(item);

        component_offset += component_width;
    }

    section.right = component_offset;

    section
}

fn clear_section(api: &Api, height: i32, left: i32, right: i32) {
    api.fill_rect(
        left,
        0,
        right - left,
        height,
        CONFIG.lock().bar.color as u32,
    )
}

fn with_item_at_pos<T: Fn(Option<&Item>) -> ()>(id: WindowId, x: i32, cb: T) {
    with_bar_by(
        |b| b.window.id == id,
        |b| {
            let mut result = None;
            if let Some(bar) = b {
                for section in vec![&bar.left, &bar.center, &bar.right] {
                    if section.left <= x && x <= section.right {
                        for item in section.items.iter() {
                            if item.left <= x && x <= item.right {
                                result = Some(item);
                                break;
                            }
                        }
                    }
                }
            }
            cb(result);
        },
    )
}

pub fn create(kb_manager: Arc<Mutex<KbManager>>) {
    info!("Creating appbar");

    let name = "nog_bar";
    let (color, height, font) = {
        let config = CONFIG.lock();

        (config.bar.color, config.bar.height, config.bar.font.clone())
    };

    spawn_refresh_thread();

    for display in DISPLAYS.lock().clone() {
        if with_bar_by(|b| b.display.id == display.id, |b| b.is_some()) {
            error!(
                "Appbar for monitor {:?} already exists. Aborting",
                display.id
            );
        } else {
            debug!("Creating appbar for display {:?}", display.id);
            let mut bar = Bar::default();

            bar.display = display;

            let left = bar.display.working_area_left();
            let top = bar.display.working_area_top() - height;
            let width = bar.display.working_area_width();

            bar.window = bar
                .window
                .with_is_popup(true)
                .with_border(false)
                .with_title(name)
                .with_font(&font)
                .with_background_color(color as u32)
                .with_pos(left, top)
                .with_size(width, height);

            let kb_manager = kb_manager.clone();

            bar.window.create(move |event| match event {
                WindowEvent::Close { id, .. } => {
                    let mut bars = BARS.lock();
                    let idx = bars.iter().position(|b| b.window.id == *id).unwrap();
                    bars.remove(idx);
                }
                WindowEvent::Click { id, x, display, .. } => with_item_at_pos(*id, *x, |item| {
                    if let Some(item) = item {
                        if item.component.is_clickable {
                            for (i, width) in item.widths.iter().enumerate() {
                                if width.0 <= *x && *x <= width.1 {
                                    item.component.on_click(display, i);
                                }
                            }
                        }
                    }
                }),
                WindowEvent::MouseMove { x, api, id, .. } => with_item_at_pos(*id, *x, |item| {
                    if let Some(item) = item {
                        if item.component.is_clickable {
                            api.set_clickable_cursor();
                            return;
                        }
                    }

                    api.set_default_cursor();
                }),
                WindowEvent::Draw { api, id, .. } => {
                    with_bar_by(
                        |b| b.window.id == *id,
                        |b| {
                            if let Some(bar) = b {
                                let grids = GRIDS.lock();
                                let workspace_id = *WORKSPACE_ID.lock();
                                let working_area_width = bar.display.working_area_width();
                                let config = CONFIG.lock();
                                let kb_manager = kb_manager.lock();
                                let left = components_to_section(
                                    api,
                                    &bar.display,
                                    &kb_manager,
                                    &grids,
                                    &config,
                                    workspace_id,
                                    &config.bar.components.left,
                                );

                                let mut center = components_to_section(
                                    api,
                                    &bar.display,
                                    &kb_manager,
                                    &grids,
                                    &config,
                                    workspace_id,
                                    &config.bar.components.center,
                                );
                                center.left = working_area_width / 2 - center.right / 2;
                                center.right += center.left;

                                let mut right = components_to_section(
                                    api,
                                    &bar.display,
                                    &kb_manager,
                                    &grids,
                                    &config,
                                    workspace_id,
                                    &config.bar.components.right,
                                );
                                right.left = working_area_width - right.right;
                                right.right += right.left;

                                draw_components(
                                    api,
                                    &bar.display,
                                    &kb_manager,
                                    &grids,
                                    &config,
                                    workspace_id,
                                    config.bar.height,
                                    left.left,
                                    &config.bar.components.left,
                                );
                                draw_components(
                                    api,
                                    &bar.display,
                                    &kb_manager,
                                    &grids,
                                    &config,
                                    workspace_id,
                                    config.bar.height,
                                    center.left,
                                    &config.bar.components.center,
                                );
                                draw_components(
                                    api,
                                    &bar.display,
                                    &kb_manager,
                                    &grids,
                                    &config,
                                    workspace_id,
                                    config.bar.height,
                                    right.left,
                                    &config.bar.components.right,
                                );

                                if bar.left.width() > left.width() {
                                    clear_section(
                                        api,
                                        config.bar.height,
                                        left.right,
                                        bar.left.right,
                                    );
                                }

                                if bar.center.width() > center.width() {
                                    let delta = (bar.center.right - center.right) / 2;
                                    clear_section(
                                        api,
                                        config.bar.height,
                                        bar.center.left,
                                        bar.center.left + delta,
                                    );
                                    clear_section(
                                        api,
                                        config.bar.height,
                                        bar.center.right - delta,
                                        bar.center.right,
                                    );
                                }

                                if bar.right.width() > right.width() {
                                    clear_section(
                                        api,
                                        config.bar.height,
                                        bar.right.left,
                                        right.left,
                                    );
                                }

                                bar.left = left;
                                bar.center = center;
                                bar.right = right;
                            }
                        },
                    );
                }
                _ => {}
            });

            BARS.lock().push(bar.clone());
        }
    }
}

#[test]
pub fn test() {
    crate::display::init();
    crate::logging::setup();
    // create();

    loop {
        thread::sleep(Duration::from_millis(1000));
    }
}
