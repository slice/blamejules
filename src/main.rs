use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use futures::stream::{self, StreamExt};
use image::imageops::FilterType;
use image::GenericImageView;
use structopt::StructOpt;
use tokio::sync::Mutex;

use blamejules::{Cmd, Sender, Vec2};

#[derive(StructOpt)]
#[structopt(name = "blamejules", about = "pixelflut client")]
struct Opt {
    /// Pixelflut server to connect to
    #[structopt(short, long)]
    server: String,

    /// Path to an image to stretch and paint onto the entire canvas
    #[structopt(long)]
    stretch_image: PathBuf,

    /// The number of futures allowed to run at once
    #[structopt(short = "j", long, default_value = "256")]
    futures: usize,

    /// Make painted images extremely low quality
    #[structopt(short, long)]
    crunch: bool,

    /// The size to resize images down to when crunching
    #[structopt(long, default_value = "16")]
    crunch_size: u32,
}

fn apply_options_to_image(
    opt: &Opt,
    Vec2(canvas_width, canvas_height): Vec2,
    mut img: image::DynamicImage,
) -> Result<image::DynamicImage> {
    if opt.crunch {
        img = img.resize_exact(opt.crunch_size, opt.crunch_size, FilterType::Nearest);
    }
    img = img.resize_exact(
        canvas_width,
        canvas_height,
        if opt.crunch {
            FilterType::Nearest
        } else {
            FilterType::Lanczos3
        },
    );
    Ok(img)
}

async fn go(opt: Opt, mut sender: Sender) -> Result<()> {
    let img = image::open(&opt.stretch_image).unwrap();
    println!(
        "opened image, dimensions: {:?}, color: {:?}",
        img.dimensions(),
        img.color()
    );

    let canvas_size = sender.query_size().await?;
    let Vec2(width, height) = canvas_size;
    println!("canvas: {}x{} ({} pixels)", width, height, width * height);

    let arc = Arc::new(Mutex::new(sender));

    println!("sending...");

    // This operation seemingly cannot fail, so just `unwrap`.
    let img = apply_options_to_image(&opt, canvas_size, img)?;
    let img_buffer = img.as_rgb8().unwrap();

    stream::iter(img_buffer.enumerate_pixels())
        .map(|(x, y, pixel)| {
            let sock = Arc::clone(&arc);
            async move {
                sock.lock()
                    .await
                    .send(Cmd::SetPx(Vec2(x, y), (*pixel).into()))
                    .await
            }
        })
        .buffer_unordered(opt.futures)
        .for_each(|result| async {
            if let Err(error) = result {
                eprintln!("fail: {}", error);
            }
        })
        .await;

    println!("done!");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();

    let addr = tokio::net::lookup_host(&opt.server)
        .await?
        .next()
        .ok_or_else(|| anyhow::anyhow!("failed to lookup server"))?;

    print!("connecting... ");
    let sender = Sender::connect(addr).await?;
    println!("connected.");

    go(opt, sender).await?;

    Ok(())
}
