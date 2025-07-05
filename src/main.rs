use std::io::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use image::ImageDecoder;

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[arg(short, long, default_value_t = 1.0)]
    distance: f32,
    #[arg(short, long, default_value_t = 7)]
    effort: u32,
    #[arg(long, default_value_t = 0)]
    decoding_speed: u32,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    force_from_pixels: bool,
    input: PathBuf,
}

fn main() {
    let args = Args::parse();

    let begin_read_image = Instant::now();
    let input_buffer = std::fs::read(args.input).unwrap();
    let duration_read_image = begin_read_image.elapsed();
    println!(
        "Reading input took {:.2} ms",
        duration_read_image.as_secs_f64() * 1000.
    );

    let image = image::ImageReader::new(std::io::Cursor::new(&input_buffer))
        .with_guessed_format()
        .unwrap();
    let is_jpeg = image.format() == Some(image::ImageFormat::Jpeg);
    let mut image = image.into_decoder().unwrap();

    let icc = image.icc_profile().unwrap();
    let (width, height) = image.dimensions();
    let (num_channels, sample_format) = {
        let color_type = image.color_type();
        let num_channels = color_type.channel_count() as u32;
        let sample_format = match color_type {
            image::ColorType::L8
            | image::ColorType::La8
            | image::ColorType::Rgb8
            | image::ColorType::Rgba8 => jexcel::SampleFormat::U8,
            image::ColorType::L16
            | image::ColorType::La16
            | image::ColorType::Rgb16
            | image::ColorType::Rgba16 => jexcel::SampleFormat::U16,
            image::ColorType::Rgb32F | image::ColorType::Rgba32F => jexcel::SampleFormat::F32,
            _ => unimplemented!(),
        };
        (num_channels, sample_format)
    };
    let bits_per_sample = {
        let color_type = image.original_color_type();
        color_type.bits_per_pixel() as u32 / color_type.channel_count() as u32
    };

    let mut encoder = jexcel::JxlEncoder::new().unwrap();

    let settings = encoder
        .create_frame_settings_with(|settings| {
            settings
                .distance(args.distance)?
                .effort(jexcel::Effort::try_from(args.effort as i64)?)
                .decoding_speed(args.decoding_speed)?;
            Ok(())
        })
        .unwrap();

    let mut transcoding_ok = false;
    let mut begin_encode = Instant::now();
    if is_jpeg && !args.force_from_pixels {
        println!("Trying to transcode...");

        begin_encode = Instant::now();
        let mut frame = encoder.add_frame(settings).unwrap();
        let jpeg_result = frame.jpeg(&input_buffer);

        if let Err(err) = jpeg_result {
            println!("Transcoding failed ({err})");
        } else {
            transcoding_ok = true;
        }
    }

    if !transcoding_ok {
        let mut basic_info = jexcel::BasicInfo::new();
        basic_info.xsize = width;
        basic_info.ysize = height;
        basic_info.bits_per_sample = bits_per_sample;
        basic_info.uses_original_profile = 1;
        encoder.set_basic_info(&basic_info).unwrap();

        if let Some(icc) = icc {
            encoder.set_icc_profile(&icc).unwrap();
        } else {
            let color_encoding = jexcel::ColorEncoding::srgb(jexcel::RenderingIntent::Relative);
            encoder.set_color_encoding(&color_encoding).unwrap();
        }

        let begin_decode_image = Instant::now();
        let mut buffer = vec![0u8; image.total_bytes() as usize];
        image.read_image(&mut buffer).unwrap();
        let duration_decode_image = begin_decode_image.elapsed();
        println!(
            "Decoding input took {:.2} ms",
            duration_decode_image.as_secs_f64() * 1000.
        );

        begin_encode = Instant::now();
        encoder
            .add_frame(settings)
            .unwrap()
            .color_channels(num_channels, sample_format, &buffer)
            .unwrap();
    }

    encoder.close_input();

    let mut buffer = vec![0u8; 4096];
    let mut output = args
        .output
        .map(|output| std::fs::File::create(output).unwrap());

    loop {
        let ret = encoder.pull_outputs(&mut buffer).unwrap();
        if let Some(output) = &mut output {
            output.write_all(&buffer[..ret.bytes_written()]).unwrap();
        }
        if !ret.need_more_output() {
            break;
        }
    }

    let duration_encode = begin_encode.elapsed();
    println!(
        "Encoding and output took {:.2} ms",
        duration_encode.as_secs_f64() * 1000.
    );
}
