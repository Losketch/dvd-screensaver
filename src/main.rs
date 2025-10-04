#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nannou::image;
use nannou::image::{DynamicImage, GenericImageView, ImageError};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use nannou_egui::{self, egui, Egui};
use rfd::FileDialog;
use std::env;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;

#[cfg(windows)]
use winapi::shared::minwindef::FALSE;
#[cfg(windows)]
use winapi::shared::windef::HWND;
#[cfg(windows)]
use winapi::um::winuser::{
    FindWindowW, GetClientRect, GetWindowLongPtrW, MoveWindow, SetParent, SetWindowLongPtrW,
    GWL_STYLE, WS_CHILD, WS_VISIBLE,
};

lazy_static::lazy_static! {
    static ref LAST_HUE: Mutex<i32> = Mutex::new(0);
}

static PREVIEW_RUNNING: AtomicBool = AtomicBool::new(false);
static mut PREVIEW_PARENT_HWND: Option<isize> = None;

struct ConfigModel {
    egui: Egui,
    config: ScreenSaverConfig,
    image_names: Vec<String>,
    custom_image_path: String,
    file_dialog_receiver: Option<mpsc::Receiver<Option<String>>>,
    is_file_dialog_open: bool,
    should_exit: bool,
}

struct Model {
    image: DynamicImage,
    original_image: DynamicImage,
    dvd_rect: Rect,
    dvd_vel: Vec2,
    m_pos: Option<Vec2>,
    is_preview: bool,
    #[allow(dead_code)]
    preview_parent: Option<isize>,
}

#[derive(Clone)]
struct ScreenSaverConfig {
    speed: f32,
    image_index: usize,
    size_factor: f32,
    custom_image_path: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        nannou::app(model)
            .update(update)
            .loop_mode(nannou::LoopMode::Rate { 
                update_interval: std::time::Duration::from_secs_f64(1.0 / 60.0) 
            })
            .run();
        return;
    }

    let flag = &args[1].to_lowercase();

    if flag.starts_with("/c") || flag.starts_with("-c") {
        show_configuration_dialog();
    } else if flag.starts_with("/p") || flag.starts_with("-p") {
        let hwnd = parse_preview_hwnd(&args);
        run_preview_mode(hwnd);
    } else if flag.starts_with("/s") || flag.starts_with("-s") {
        nannou::app(model).update(update).run();
    } else if flag.starts_with("/a") || flag.starts_with("-a") {
        std::process::exit(0);
    } else {
        nannou::app(model).update(update).run();
    }
}

fn parse_preview_hwnd(args: &[String]) -> Option<isize> {
    if args.len() > 2 {
        args[2].parse::<isize>().ok()
    } else if args.len() > 1 {
        let flag = &args[1];
        if flag.len() > 2 {
            flag[2..].parse::<isize>().ok()
        } else {
            None
        }
    } else {
        None
    }
}

fn run_preview_mode(hwnd: Option<isize>) {
    if PREVIEW_RUNNING.swap(true, Ordering::SeqCst) {
        std::process::exit(0);
    }

    unsafe {
        PREVIEW_PARENT_HWND = hwnd;
    }

    if hwnd.is_some() {
        nannou::app(preview_model_embedded).update(update).run();
    } else {
        nannou::app(preview_model_standalone).update(update).run();
    }

    PREVIEW_RUNNING.store(false, Ordering::SeqCst);
}

#[cfg(windows)]
fn preview_model_embedded(app: &App) -> Model {
    let parent_hwnd = unsafe { PREVIEW_PARENT_HWND };

    let _window_id = app
        .new_window()
        .size(200, 150)
        .title("DVD Screensaver Preview")
        .event(window_event)
        .view(view)
        .decorations(false)
        .resizable(false)
        .build()
        .unwrap();

    if let Some(parent_hwnd) = parent_hwnd {
        thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(200));

            unsafe {
                let window_title = std::ffi::CString::new("DVD Screensaver Preview").unwrap();
                let mut title_wide: Vec<u16> =
                    window_title.to_string_lossy().encode_utf16().collect();
                title_wide.push(0);

                let child_hwnd = FindWindowW(std::ptr::null(), title_wide.as_ptr());

                if !child_hwnd.is_null() {
                    let parent = parent_hwnd as HWND;

                    let mut client_rect = std::mem::zeroed();
                    if GetClientRect(parent, &mut client_rect) != FALSE {
                        let width = client_rect.right - client_rect.left;
                        let height = client_rect.bottom - client_rect.top;

                        let _current_style = GetWindowLongPtrW(child_hwnd, GWL_STYLE);

                        SetParent(child_hwnd, parent);

                        let new_style = (WS_CHILD | WS_VISIBLE) as isize;
                        SetWindowLongPtrW(child_hwnd, GWL_STYLE, new_style);

                        MoveWindow(child_hwnd, 0, 0, width, height, 1);
                    }
                }
            }
        });
    }

    create_preview_model(true, parent_hwnd)
}

#[cfg(not(windows))]
fn preview_model_embedded(app: &App) -> Model {
    preview_model_standalone(app)
}

fn preview_model_standalone(app: &App) -> Model {
    let _window_id = app
        .new_window()
        .size(200, 150)
        .title("DVD Screensaver Preview")
        .event(window_event)
        .view(view)
        .decorations(true)
        .always_on_top(true)
        .resizable(false)
        .msaa_samples(4)
        .build()
        .unwrap();

    create_preview_model(true, None)
}

fn create_preview_model(is_preview: bool, parent_hwnd: Option<isize>) -> Model {
    let config = load_config();

    let preview_size = if parent_hwnd.is_some() {
        (100.0, 75.0)
    } else {
        (200.0, 150.0)
    };

    let original_image = match get_image_data(config.image_index, &config.custom_image_path) {
        Ok(img) => {
            let target_width = (preview_size.0 * config.size_factor * 2.0) as u32;
            let target_height = (preview_size.1 * config.size_factor * 2.0) as u32;

            img.thumbnail(target_width.max(40), target_height.max(30))
        }
        Err(_) => {
            let data = include_bytes!("../assets/dvd_logo.png");
            let default_img = image::load_from_memory(data).expect("Unable to load default icon");

            let target_width = (preview_size.0 * config.size_factor * 2.0) as u32;
            let target_height = (preview_size.1 * config.size_factor * 2.0) as u32;

            default_img.thumbnail(target_width.max(40), target_height.max(30))
        }
    };

    let image = change_color(&original_image);

    let rect = Rect::from_x_y_w_h(
        0.0,
        0.0,
        image.dimensions().0 as f32,
        image.dimensions().1 as f32,
    );

    Model {
        image,
        original_image,
        dvd_rect: rect,
        dvd_vel: Vec2::new(config.speed * 0.5, config.speed * 0.5),
        m_pos: None,
        is_preview,
        preview_parent: parent_hwnd,
    }
}

fn show_configuration_dialog() {
    nannou::app(config_model)
        .update(config_update)
        .view(config_view)
        .run();
}

fn config_model(app: &App) -> ConfigModel {
    let _window_id = app
        .new_window()
        .size(500, 400)
        .title("DVD Screensaver Configuration")
        .view(config_view)
        .raw_event(raw_window_event)
        .build()
        .unwrap();

    let window = app.window(_window_id).unwrap();
    let egui = Egui::from_window(&window);

    let config = load_config();
    let image_names = vec![
        "Built-in DVD Logo".to_string(),
        "Built-in DVD Logo 2".to_string(),
        "Custom Icon".to_string(),
    ];

    ConfigModel {
        egui,
        config: config.clone(),
        image_names,
        custom_image_path: config.custom_image_path,
        file_dialog_receiver: None,
        is_file_dialog_open: false,
        should_exit: false,
    }
}

fn config_update(_app: &App, model: &mut ConfigModel, update: Update) {
    if model.should_exit {
        std::process::exit(0);
    }

    let egui = &mut model.egui;
    egui.set_elapsed_time(update.since_start);

    if let Some(receiver) = &model.file_dialog_receiver {
        if let Ok(result) = receiver.try_recv() {
            model.is_file_dialog_open = false;
            if let Some(path) = result {
                model.custom_image_path = path;
                model.config.custom_image_path = model.custom_image_path.clone();
            }
            model.file_dialog_receiver = None;
        }
    }

    let ctx = egui.begin_frame();

    let mut fonts = egui::FontDefinitions::default();

    if let Ok(font_data) = std::fs::read("C:/Windows/Fonts/segoeui.ttf") {
        fonts
            .font_data
            .insert("Segoe UI".to_owned(), egui::FontData::from_owned(font_data));

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Segoe UI".to_owned());

        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("Segoe UI".to_owned());

        ctx.set_fonts(fonts);
    }

    let mut window_open = true;
    egui::Window::new("DVD Screensaver Settings")
        .default_size([450.0, 350.0])
        .open(&mut window_open)
        .show(&ctx, |ui| {
            ui.heading("Movement Speed");
            ui.add(egui::Slider::new(&mut model.config.speed, 10.0..=200.0).text("pixels/second"));

            ui.separator();

            ui.heading("Icon Selection");
            egui::ComboBox::from_label("Select Icon")
                .selected_text(
                    &model.image_names[model.config.image_index.min(model.image_names.len() - 1)],
                )
                .show_ui(ui, |ui| {
                    for (i, name) in model.image_names.iter().enumerate() {
                        ui.selectable_value(&mut model.config.image_index, i, name);
                    }
                });

            if model.config.image_index == 2 {
                ui.label("Custom icon path:");
                ui.text_edit_singleline(&mut model.custom_image_path);

                ui.horizontal(|ui| {
                    let button_text = if model.is_file_dialog_open {
                        "File dialog is open..."
                    } else {
                        "Browse File"
                    };

                    if ui
                        .add_enabled(!model.is_file_dialog_open, egui::Button::new(button_text))
                        .clicked()
                    {
                        let (sender, receiver) = mpsc::channel();
                        model.file_dialog_receiver = Some(receiver);
                        model.is_file_dialog_open = true;

                        thread::spawn(move || {
                            let result = FileDialog::new()
                                .add_filter(
                                    "Image Files",
                                    &[
                                        "png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif",
                                        "webp",
                                    ],
                                )
                                .add_filter("PNG Files", &["png"])
                                .add_filter("JPEG Files", &["jpg", "jpeg"])
                                .add_filter("GIF Files", &["gif"])
                                .add_filter("BMP Files", &["bmp"])
                                .add_filter("ICO Files", &["ico"])
                                .add_filter("TIFF Files", &["tiff", "tif"])
                                .add_filter("WebP Files", &["webp"])
                                .add_filter("All Files", &["*"])
                                .set_title("Select Icon File")
                                .pick_file();

                            let path_string = result.map(|path| path.to_string_lossy().to_string());
                            let _ = sender.send(path_string);
                        });
                    }

                    ui.label("Supported formats: PNG, JPG, GIF, BMP, ICO, TIFF, WebP");
                });

                if !model.custom_image_path.is_empty() {
                    let path = Path::new(&model.custom_image_path);
                    if path.exists() {
                        if let Some(extension) = path.extension() {
                            let ext = extension.to_string_lossy().to_lowercase();
                            let supported_formats = [
                                "png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp",
                            ];
                            if supported_formats.contains(&ext.as_str()) {
                                ui.colored_label(
                                    egui::Color32::GREEN,
                                    "✓ File exists and format is supported",
                                );
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 165, 0),
                                    "⚠ File exists but format may not be supported",
                                );
                            }
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 165, 0),
                                "⚠ File exists but has no extension",
                            );
                        }
                    } else {
                        ui.colored_label(egui::Color32::RED, "✗ File does not exist");
                    }
                }

                model.config.custom_image_path = model.custom_image_path.clone();

                ui.separator();
                ui.label("Quick select:");
                ui.horizontal(|ui| {
                    if ui.small_button("Desktop").clicked() {
                        if let Some(desktop) = dirs::desktop_dir() {
                            model.custom_image_path = desktop.to_string_lossy().to_string();
                        }
                    }
                    if ui.small_button("Pictures").clicked() {
                        if let Some(pictures) = dirs::picture_dir() {
                            model.custom_image_path = pictures.to_string_lossy().to_string();
                        }
                    }
                    if ui.small_button("Downloads").clicked() {
                        if let Some(downloads) = dirs::download_dir() {
                            model.custom_image_path = downloads.to_string_lossy().to_string();
                        }
                    }
                });
            }

            ui.separator();

            ui.heading("Icon Size");
            ui.add(
                egui::Slider::new(&mut model.config.size_factor, 0.05..=0.5)
                    .text("size multiplier"),
            );

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Save and Exit").clicked() {
                    save_config(&model.config);
                    model.should_exit = true;
                }

                if ui.button("Cancel").clicked() {
                    model.should_exit = true;
                }

                if ui.button("Reset to Default").clicked() {
                    model.config = ScreenSaverConfig {
                        speed: 50.0,
                        image_index: 0,
                        size_factor: 0.16,
                        custom_image_path: String::new(),
                    };
                    model.custom_image_path = String::new();
                }
            });

            ui.separator();
            ui.small("Tip: Drag image files to the path field to quickly set the path");
        });

    if !window_open {
        model.should_exit = true;
    }
}

fn config_view(_app: &App, model: &ConfigModel, frame: Frame) {
    frame.clear(nannou::color::rgb(0.1, 0.1, 0.1));
    model.egui.draw_to_frame(&frame).unwrap();
}

fn raw_window_event(
    _app: &App,
    model: &mut ConfigModel,
    event: &nannou::winit::event::WindowEvent,
) {
    model.egui.handle_raw_event(event);

    if let nannou::winit::event::WindowEvent::CloseRequested = event {
        model.should_exit = true;
    }
}

fn get_config_path() -> PathBuf {
    if let Some(appdata) = dirs::config_dir() {
        let config_dir = appdata.join("DVDScreensaver");
        let _ = create_dir_all(&config_dir);
        config_dir.join("config.ini")
    } else {
        PathBuf::from("screensaver.ini")
    }
}

fn load_config() -> ScreenSaverConfig {
    let config_path = get_config_path();

    if config_path.exists() {
        if let Ok(mut file) = File::open(&config_path) {
            let mut contents = String::new();
            if file.read_to_string(&mut contents).is_ok() {
                let mut lines = contents.lines();
                let speed = lines.next().unwrap_or("50.0").parse().unwrap_or(50.0);
                let image_index = lines.next().unwrap_or("0").parse().unwrap_or(0);
                let size_factor = lines.next().unwrap_or("0.16").parse().unwrap_or(0.16);
                let custom_image_path = lines.next().unwrap_or("").to_string();
                return ScreenSaverConfig {
                    speed,
                    image_index,
                    size_factor,
                    custom_image_path,
                };
            }
        }
    }

    ScreenSaverConfig {
        speed: 50.0,
        image_index: 0,
        size_factor: 0.16,
        custom_image_path: String::new(),
    }
}

fn save_config(config: &ScreenSaverConfig) {
    let config_path = get_config_path();

    if let Some(parent) = config_path.parent() {
        let _ = create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&config_path)
    {
        let _ = writeln!(file, "{}", config.speed);
        let _ = writeln!(file, "{}", config.image_index);
        let _ = writeln!(file, "{}", config.size_factor);
        let _ = writeln!(file, "{}", config.custom_image_path);
    }
}

fn load_image_safe(path: &str) -> Result<DynamicImage, ImageError> {
    if path.is_empty() {
        return Err(ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Empty path",
        )));
    }

    let path = Path::new(path);
    if !path.exists() {
        return Err(ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found",
        )));
    }

    image::open(path)
}

fn get_image_data(image_index: usize, custom_path: &str) -> Result<DynamicImage, String> {
    match image_index {
        0 => {
            let data = include_bytes!("../assets/dvd_logo.png");
            image::load_from_memory(data)
                .map_err(|e| format!("Unable to load built-in icon 1: {}", e))
        }
        1 => {
            let data = include_bytes!("../assets/dvd_logo2.png");
            image::load_from_memory(data)
                .map_err(|e| format!("Unable to load built-in icon 2: {}", e))
        }
        2 => {
            if custom_path.is_empty() {
                return Err("No custom icon path specified".to_string());
            }

            load_image_safe(custom_path)
                .map_err(|e| format!("Unable to load custom icon '{}': {}", custom_path, e))
        }
        _ => {
            let data = include_bytes!("../assets/dvd_logo.png");
            image::load_from_memory(data).map_err(|e| format!("Unable to load default icon: {}", e))
        }
    }
}

fn change_color(image: &DynamicImage) -> DynamicImage {
    let mut rng = thread_rng();
    let mut last_hue = LAST_HUE.lock().unwrap();

    let mut new_hue;
    loop {
        new_hue = rng.gen_range(0..360);
        let hue_diff = (new_hue - *last_hue).abs();
        let min_diff = hue_diff.min(360 - hue_diff);

        if min_diff >= 60 {
            break;
        }
    }

    *last_hue = new_hue;
    drop(last_hue);

    image.huerotate(new_hue).brighten(10).adjust_contrast(1.2)
}

fn model(app: &App) -> Model {
    let _primary_window_id = app
        .new_window()
        .event(window_event)
        .view(view)
        .fullscreen()
        .msaa_samples(4)
        .build()
        .unwrap();

    let primary_window = app.window(_primary_window_id).unwrap();
    primary_window.set_cursor_visible(false);

    let config = load_config();

    let original_image = match get_image_data(config.image_index, &config.custom_image_path) {
        Ok(img) => {
            let window_rect = app.window_rect();
            let target_width = (window_rect.w() * config.size_factor) as u32;
            let target_height = (window_rect.h() * config.size_factor) as u32;

            img.thumbnail(target_width, target_height)
        }
        Err(error) => {
            eprintln!("Icon loading failed: {}, using default icon", error);

            let data = include_bytes!("../assets/dvd_logo.png");
            let default_img = image::load_from_memory(data).expect("Unable to load default icon");

            let window_rect = app.window_rect();
            let target_width = (window_rect.w() * config.size_factor) as u32;
            let target_height = (window_rect.h() * config.size_factor) as u32;

            default_img.thumbnail(target_width, target_height)
        }
    };

    let image = change_color(&original_image);

    let rect = Rect::from_x_y_w_h(
        0.0,
        0.0,
        image.dimensions().0 as f32,
        image.dimensions().1 as f32,
    );

    Model {
        image,
        original_image,
        dvd_rect: rect,
        dvd_vel: Vec2::new(config.speed, config.speed),
        m_pos: None,
        is_preview: false,
        preview_parent: None,
    }
}

fn window_event(app: &App, model: &mut Model, event: WindowEvent) {
    if model.is_preview {
        return;
    }

    if app.time > 0.1 {
        match event {
            WindowEvent::MouseMoved(pos) => {
                if model.m_pos.is_none() {
                    model.m_pos = Some(pos);
                }
                if model.m_pos.unwrap() != pos {
                    app.quit();
                }
            }
            WindowEvent::MousePressed(..)
            | WindowEvent::KeyPressed(..)
            | WindowEvent::MouseWheel(..) => app.quit(),
            _ => (),
        }
    }
}

fn update(app: &App, model: &mut Model, _update: Update) {
    let win = app.window_rect();
    let delta_time = app.duration.since_prev_update.secs() as f32;
    let dvd_vel = &mut model.dvd_vel;

    let new_x = model.dvd_rect.x() + dvd_vel.x * delta_time;
    let new_y = model.dvd_rect.y() + dvd_vel.y * delta_time;

    model.dvd_rect = Rect::from_x_y_w_h(new_x, new_y, model.dvd_rect.w(), model.dvd_rect.h());

    let mut color_changed = false;

    if model.dvd_rect.left() <= win.left() {
        model.dvd_rect = Rect::from_x_y_w_h(
            win.left() + model.dvd_rect.w() / 2.0,
            model.dvd_rect.y(),
            model.dvd_rect.w(),
            model.dvd_rect.h(),
        );
        dvd_vel.x = dvd_vel.x.abs();
        color_changed = true;
    }

    if model.dvd_rect.right() >= win.right() {
        model.dvd_rect = Rect::from_x_y_w_h(
            win.right() - model.dvd_rect.w() / 2.0,
            model.dvd_rect.y(),
            model.dvd_rect.w(),
            model.dvd_rect.h(),
        );
        dvd_vel.x = -dvd_vel.x.abs();
        color_changed = true;
    }

    if model.dvd_rect.bottom() <= win.bottom() {
        model.dvd_rect = Rect::from_x_y_w_h(
            model.dvd_rect.x(),
            win.bottom() + model.dvd_rect.h() / 2.0,
            model.dvd_rect.w(),
            model.dvd_rect.h(),
        );
        dvd_vel.y = dvd_vel.y.abs();
        color_changed = true;
    }

    if model.dvd_rect.top() >= win.top() {
        model.dvd_rect = Rect::from_x_y_w_h(
            model.dvd_rect.x(),
            win.top() - model.dvd_rect.h() / 2.0,
            model.dvd_rect.w(),
            model.dvd_rect.h(),
        );
        dvd_vel.y = -dvd_vel.y.abs();
        color_changed = true;
    }

    if color_changed {
        model.image = change_color(&model.original_image);
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    frame.clear(BLACK);

    let draw = app.draw();
    let texture = wgpu::Texture::from_image(app, &model.image);

    draw.texture(&texture)
        .xy(model.dvd_rect.xy())
        .wh(model.dvd_rect.wh());

    draw.to_frame(app, &frame).unwrap();
}
