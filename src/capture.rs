//! Opt-in development capture of this application's own framebuffer.
//! Avoids macOS screen-recording permission and never captures other windows.
use eframe::egui;
use std::{
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
static FRAME: AtomicUsize = AtomicUsize::new(0);
pub fn tick(ctx: &egui::Context, ready: bool) {
    let Some(path) = std::env::var_os("POST_RONDA_CAPTURE") else {
        return;
    };
    let screenshot = ctx.input(|input| {
        input.events.iter().find_map(|event| {
            if let egui::Event::Screenshot { image, .. } = event {
                Some(image.clone())
            } else {
                None
            }
        })
    });
    if let Some(image) = screenshot {
        let mut file = std::fs::File::create(&path).expect("capture output");
        write!(file, "P6\n{} {}\n255\n", image.size[0], image.size[1]).unwrap();
        let bytes: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p| {
                let c = p.to_array();
                [c[0], c[1], c[2]]
            })
            .collect();
        file.write_all(&bytes).unwrap();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    }
    if std::env::var_os("POST_RONDA_DEMO_SEND").is_some() && !ready {
        ctx.request_repaint_after(std::time::Duration::from_millis(20));
        return;
    }
    let wait = if let Some(frames) = std::env::var("POST_RONDA_CAPTURE_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        frames
    } else if std::env::var_os("POST_RONDA_DRAG").is_some() {
        16
    } else if std::env::var_os("POST_RONDA_CLICK").is_some() {
        12
    } else {
        3
    };
    if FRAME.fetch_add(1, Ordering::Relaxed) == wait {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }
    ctx.request_repaint();
}
