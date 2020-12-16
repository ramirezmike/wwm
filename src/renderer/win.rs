use super::Renderer;
use crate::{
    config::Config, display::Display, system::SystemError, system::SystemResult, 
    tile_grid::TileGrid, system::NativeWindow
};
use winapi::{shared::windef::*, um::winuser::*};

#[derive(Default, Clone, Copy, Debug)]
pub struct WinRenderer;

impl Renderer for WinRenderer {
    fn render<TRenderer: Renderer>(
        &self,
        grid: &TileGrid<TRenderer>,
        window: &NativeWindow,
        config: &Config,
        display: &Display,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> SystemResult {
        let rule = window.rule.clone().unwrap_or_default();

        let mut left = x;
        let mut right = x + width;
        let mut top = y;
        let mut bottom = y + height;

        let use_dpi: bool;

        unsafe {
            let border_width = match GetSystemMetricsForDpi(SM_CXFRAME, display.dpi) {
                                  0 => {
                                      use_dpi = false;
                                      GetSystemMetrics(SM_CXFRAME)
                                  },
                                  x @ _ => {
                                      use_dpi = true;
                                      x
                                  }
                               };
            let border_height = if use_dpi { GetSystemMetricsForDpi(SM_CYFRAME, display.dpi) } 
                                else { GetSystemMetrics(SM_CYFRAME) };

            if rule.chromium || rule.firefox || !config.remove_title_bar {
                let caption_height = if use_dpi { GetSystemMetricsForDpi(SM_CYCAPTION, display.dpi) }
                                     else { GetSystemMetrics(SM_CYCAPTION) };
                top += caption_height;
            } else {
                top -= border_height * 2;

                if config.use_border {
                    left += 1;
                    right -= 1;
                    top += 1;
                    bottom -= 1;
                }
            }

            if rule.firefox
                || rule.chromium
                || (!config.remove_title_bar && rule.has_custom_titlebar)
            {
                if rule.firefox || rule.chromium {
                    let mut clientRect = RECT { bottom: 0, left: 0, right: 0, top: 0 };
                    GetClientRect(window.id.into(), &mut clientRect);
                    let mut windowRect = RECT { bottom: 0, left: 0, right: 0, top: 0 };
                    GetWindowRect(window.id.into(), &mut windowRect);

                    if rule.firefox {
                        top += (((windowRect.bottom - windowRect.top) - clientRect.bottom) as f32 * 1.0) as i32;
                        left += ((windowRect.right - windowRect.left) - clientRect.right);
                        right -= ((windowRect.right - windowRect.left) - clientRect.right);
                    } else if rule.chromium {
                        top += (windowRect.bottom - windowRect.top) - clientRect.bottom;
                    }
                    /*
                    left -= ((windowRect.right - windowRect.left) - clientRect.right) / 4;
                    right += ((windowRect.right - windowRect.left) - clientRect.right) / 4;
                    bottom += ((windowRect.bottom - windowRect.top) - clientRect.bottom) / 4;
                    */
                } else {
                    left += border_width * 2;
                    right -= border_width * 2;
                    top += border_height * 2;
                    bottom -= border_height * 2;
                }
            } else {
                top += border_height * 2;
            }
        }

        let mut rect = RECT {
            left,
            right,
            top,
            bottom,
        };

        // println!("before {}", rect_to_string(rect));

        unsafe {
            if use_dpi {
                AdjustWindowRectExForDpi(
                    &mut rect,
                    window.style.bits() as u32,
                    0,
                    window.exstyle.bits() as u32,
                    display.dpi
                );
            } else {
                AdjustWindowRectEx(
                    &mut rect,
                    window.style.bits() as u32,
                    0,
                    window.exstyle.bits() as u32,
                );
            }
        }

        // println!("after {}", rect_to_string(rect));

        window.set_window_pos(rect.into(), None, Some(SWP_NOSENDCHANGING))
              .map_err(SystemError::DrawTile)
    }
}
