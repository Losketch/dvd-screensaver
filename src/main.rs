#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nannou::image;
use nannou::prelude::*;
use nannou_egui::{self, egui, Egui};
use nannou::image::{DynamicImage, GenericImageView, ImageError};
use nannou::rand::{thread_rng, Rng};
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use rfd::FileDialog;
use std::sync::mpsc;
use std::thread;

struct ConfigModel {
    egui: Egui,
    config: ScreenSaverConfig,
    image_names: Vec<String>,
    custom_image_path: String,
    file_dialog_receiver: Option<mpsc::Receiver<Option<String>>>,
    is_file_dialog_open: bool,
}

struct Model {
    image: DynamicImage,
    dvd_rect: Rect,
    dvd_vel: Vec2,
    m_pos: Option<Vec2>,
}

#[derive(Clone)]
struct ScreenSaverConfig {
    speed: f32,
    image_index: usize,
    size_factor: f32,
    custom_image_path: String,
}

fn main() {
    let flag = env::args().nth(1).unwrap_or_default();
    if flag.starts_with("/c") {
        show_configuration_dialog();
    } else if flag.starts_with("/p") {
        nannou::app(model).update(update).run();
    } else {
        nannou::app(model).update(update).run();
    }
}

fn show_configuration_dialog() {
    nannou::app(config_model)
        .update(config_update)
        .view(config_view)
        .run();
}

fn config_model(app: &App) -> ConfigModel {
    let window_id = app
        .new_window()
        .size(500, 400)
        .title("DVD Screensaver Configuration")
        .view(config_view)
        .raw_event(raw_window_event)
        .build()
        .unwrap();

    let window = app.window(window_id).unwrap();
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
    }
}

fn config_update(app: &App, model: &mut ConfigModel, update: Update) {
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
        fonts.font_data.insert(
            "Segoe UI".to_owned(),
            egui::FontData::from_owned(font_data),
        );

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

    egui::Window::new("DVD Screensaver Settings")
        .default_size([450.0, 350.0])
        .show(&ctx, |ui| {
            ui.heading("Movement Speed");
            ui.add(egui::Slider::new(&mut model.config.speed, 10.0..=200.0)
                .text("pixels/second"));

            ui.separator();

            ui.heading("Icon Selection");
            egui::ComboBox::from_label("Select Icon")
                .selected_text(&model.image_names[model.config.image_index.min(model.image_names.len() - 1)])
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

                    if ui.add_enabled(!model.is_file_dialog_open, egui::Button::new(button_text)).clicked() {
                        let (sender, receiver) = mpsc::channel();
                        model.file_dialog_receiver = Some(receiver);
                        model.is_file_dialog_open = true;

                        thread::spawn(move || {
                            let result = FileDialog::new()
                                .add_filter("Image Files", &["png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp"])
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
                            let supported_formats = ["png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp"];
                            if supported_formats.contains(&ext.as_str()) {
                                ui.colored_label(egui::Color32::GREEN, "✓ File exists and format is supported");
                            } else {
                                ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "⚠ File exists but format may not be supported");
                            }
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "⚠ File exists but has no extension");
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
            ui.add(egui::Slider::new(&mut model.config.size_factor, 0.05..=0.5)
                .text("size multiplier"));

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Save and Exit").clicked() {
                    save_config(&model.config);
                    app.quit();
                }

                if ui.button("Cancel").clicked() {
                    app.quit();
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
}

fn config_view(_app: &App, model: &ConfigModel, frame: Frame) {
    model.egui.draw_to_frame(&frame).unwrap();
}

fn raw_window_event(_app: &App, model: &mut ConfigModel, event: &nannou::winit::event::WindowEvent) {
    model.egui.handle_raw_event(event);
}

fn load_config() -> ScreenSaverConfig {
    let config_path = Path::new("screensaver.ini");
    if config_path.exists() {
        if let Ok(mut file) = File::open(config_path) {
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
                    custom_image_path 
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
    if let Ok(mut file) = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("screensaver.ini")
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
            "Empty path"
        )));
    }

    let path = Path::new(path);
    if !path.exists() {
        return Err(ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found"
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
        },
        1 => {
            let data = include_bytes!("../assets/dvd_logo2.png");
            image::load_from_memory(data)
                .map_err(|e| format!("Unable to load built-in icon 2: {}", e))
        },
        2 => {
            if custom_path.is_empty() {
                return Err("No custom icon path specified".to_string());
            }
            
            load_image_safe(custom_path)
                .map_err(|e| format!("Unable to load custom icon '{}': {}", custom_path, e))
        },
        _ => {
            let data = include_bytes!("../assets/dvd_logo.png");
            image::load_from_memory(data)
                .map_err(|e| format!("Unable to load default icon: {}", e))
        }
    }
}

fn change_color(image: &DynamicImage) -> DynamicImage {
    image.huerotate(thread_rng().gen_range(120..240))
}

fn model(app: &App) -> Model {
    let primary_window_id = app
        .new_window()
        .event(window_event)
        .view(view)
        .fullscreen()
        .build()
        .unwrap();

    let primary_window = app.window(primary_window_id).unwrap();
    primary_window.set_cursor_visible(false);

    let config = load_config();

    let image = match get_image_data(config.image_index, &config.custom_image_path) {
        Ok(img) => {
            let window_rect = app.window_rect();
            let target_width = (window_rect.w() * config.size_factor) as u32;
            let target_height = (window_rect.h() * config.size_factor) as u32;
            
            change_color(&img.thumbnail(target_width, target_height))
        },
        Err(error) => {
            eprintln!("Icon loading failed: {}, using default icon", error);

            let data = include_bytes!("../assets/dvd_logo.png");
            let default_img = image::load_from_memory(data)
                .expect("Unable to load default icon");

            let window_rect = app.window_rect();
            let target_width = (window_rect.w() * config.size_factor) as u32;
            let target_height = (window_rect.h() * config.size_factor) as u32;
            
            change_color(&default_img.thumbnail(target_width, target_height))
        }
    };

    let rect = Rect::from_x_y_w_h(
        0.0,
        0.0,
        image.dimensions().0 as f32,
        image.dimensions().1 as f32,
    );

    Model {
        image,
        dvd_rect: rect,
        dvd_vel: Vec2::new(config.speed, config.speed),
        m_pos: None,
    }
}

fn window_event(app: &App, model: &mut Model, event: WindowEvent) {
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

    model.dvd_rect = model
        .dvd_rect
        .shift_x(dvd_vel.x * delta_time)
        .shift_y(dvd_vel.y * delta_time);

    if model.dvd_rect.left() <= win.left() || model.dvd_rect.right() >= win.right() {
        dvd_vel.x = -dvd_vel.x;
        model.image = change_color(&model.image);
    }
    if model.dvd_rect.bottom() <= win.bottom() || model.dvd_rect.top() >= win.top() {
        dvd_vel.y = -dvd_vel.y;
        model.image = change_color(&model.image);
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let texture = wgpu::Texture::from_image(app, &model.image);

    draw.texture(&texture)
        .xy(model.dvd_rect.xy())
        .wh(model.dvd_rect.wh());

    frame.clear(BLACK);
    draw.to_frame(app, &frame).unwrap();
}
