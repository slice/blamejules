use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use image::imageops::FilterType;
use image::GenericImageView;
use structopt::StructOpt;

use blamejules::{Cmd, Rgb, Sender, Vec2};

#[derive(StructOpt)]
#[structopt(name = "blamejules", about = "pixelflut client")]
struct Opt {
    /// Pixelflut server to connect to
    #[structopt(short, long)]
    server: String,

    /// Path to an image to stretch and paint onto the entire canvas
    #[structopt(long)]
    stretch_image: PathBuf,

    /// The number of simultaneous connections used to paint pixels
    #[structopt(short = "c", long, default_value = "4")]
    connections: usize,

    /// How many evenly-sized chunks to split the image into and concurrently
    /// schedule pixel paints from
    #[structopt(short = "k", long, default_value = "4")]
    chunks: u32,

    /// Make painted images extremely low quality
    #[structopt(long)]
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

    let canvas_size = sender.sock.query_size().await?;
    let Vec2(width, height) = canvas_size;
    let total_size = width * height;
    println!("canvas: {}x{} ({} pixels)", width, height, total_size);

    let img = apply_options_to_image(&opt, canvas_size, img)?;
    let img_buffer = img.to_rgb8();

    let pixels: Vec<(Vec2, Rgb)> = img_buffer
        .enumerate_pixels()
        .map(|(x, y, color)| (Vec2(x, y), (*color).into()))
        .collect();

    let arc = Arc::new(sender);

    // Evenly divide the image into chunks.
    let chunk_size: usize = (total_size / opt.chunks).try_into().unwrap();
    let chunks = pixels.chunks(chunk_size);

    println!(
        "sending (chunks: {}, chunk size: {})...",
        opt.chunks, chunk_size
    );

    async fn send_chunk(sender: Arc<Sender>, chunk: &[(Vec2, Rgb)]) {
        for (coordinate, pixel) in chunk {
            if let Err(err) = sender.send(Cmd::SetPx(*coordinate, *pixel)).await {
                eprintln!(
                    "failed to paint pixel @ {:?} with {:?}, because: {:?}",
                    coordinate, pixel, err
                );
            }
        }
    }

    // Concurrently send the pixels from each chunk.
    let futures = chunks.map(|chunk| {
        let sender = Arc::clone(&arc);
        async move {
            send_chunk(sender, chunk).await;
        }
    });

    // Wait for all chunks to finish sending pixels.
    futures::future::join_all(futures).await;

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

    print!("connecting ({} + 1 sockets)... ", opt.connections);
    let sender = Sender::connect(addr, opt.connections).await?;
    println!("connected.");

    go(opt, sender).await?;

    Ok(())
}
