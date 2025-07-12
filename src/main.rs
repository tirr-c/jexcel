use std::io::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use image::ImageDecoder;

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Encoding distance. Value of 0 triggers lossless encoding.
    ///
    /// Corresponds to cjxl `-d`.
    #[arg(short, long)]
    distance: Option<f32>,
    /// Encoding effort.
    ///
    /// Corresponds to cjxl `-e`.
    #[arg(short, long, value_parser = 1..=10, default_value_t = 7)]
    effort: i64,
    /// Encode progressive image.
    ///
    /// Progressiveness increases when given multiple times.
    #[arg(short, long, action = clap::ArgAction::Count)]
    progressive: u8,
    /// Speed tier when decoding output image.
    ///
    /// Corresponds to cjxl `--faster_decoding`.
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..=4), default_value_t = 0)]
    decoding_speed: u32,
    /// Forces Modular frame.
    ///
    /// This will encode lossy Modular image when used with positive distance settings.
    #[arg(short = 'm', long)]
    force_modular: bool,
    /// Output file name.
    ///
    /// If not given, it will write nothing and work like cjxl `--disable_output`.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Whether to disable lossless JPEG transcoding and force encoding from pixels.
    #[arg(long)]
    force_from_pixels: bool,
    /// Input file name.
    input: PathBuf,
}

fn main() {
    let args = Args::parse();
    let mut distance = args
        .distance
        .unwrap_or(if args.force_modular { 0. } else { 1. });
    let is_lossless = distance < 0.01;
    let effort = jexcel::Effort::try_from(args.effort).unwrap();
    if is_lossless {
        distance = 0.;
    }
    let is_modular = is_lossless || args.force_modular;

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
    let format = image.format();
    let is_jpeg = image.format() == Some(image::ImageFormat::Jpeg);
    let do_transcode = is_jpeg && !args.force_from_pixels;
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

    let mut modular_responsive = None;
    let mut lf_frames = None;
    let mut progressive_hf = None;
    let mut progressive_hf_q = None;

    if !do_transcode && args.progressive > 0 {
        if is_modular {
            modular_responsive = Some(true);
        } else {
            lf_frames = Some(if args.progressive >= 4 { 2u32 } else { 1u32 });

            if args.progressive >= 2 {
                progressive_hf_q = Some(true);
            }

            if args.progressive >= 3 {
                progressive_hf = Some(true);
            }
        }
    }

    print!(
        "Input: {:?}, {width} x {height}, {bits_per_sample} bpc",
        format.unwrap()
    );
    if icc.is_some() {
        print!(", has ICC profile");
    }
    println!();

    print!("Encoding params: ");
    if do_transcode {
        print!("lossless JPEG transcode");
    } else if is_lossless {
        print!("lossless");
    } else {
        print!(
            "lossy{}, distance: {distance}",
            if is_modular { " Modular" } else { "" },
        );
    }
    print!(", effort: {} ({effort:?})", effort as i64);
    if args.decoding_speed > 0 {
        print!(", decoding speed: {}", args.decoding_speed);
    }
    if args.output.is_none() {
        print!(", no output");
    }
    println!();

    if args.progressive > 0 {
        print!("Progressiveness: ");
        if is_modular {
            print!("enabled");
        } else {
            if args.progressive >= 4 {
                print!("2 LF frames");
            } else if args.progressive >= 1 {
                print!("1 LF frame");
            }
            if args.progressive >= 2 {
                print!(", multiple HF quantization passes");
            }
            if args.progressive >= 3 {
                print!(", spectral HF progression");
            }
        }
        println!();
    }

    let mut encoder = jexcel::JxlEncoder::new().unwrap();

    let settings = encoder
        .create_frame_settings_with(|settings| {
            settings
                .distance(distance)?
                .effort(effort)
                .modular_progressive(modular_responsive)
                .vardct_progressive_lf(lf_frames)?
                .vardct_progressive_hf(progressive_hf)
                .vardct_progressive_hf_quant(progressive_hf_q)
                .modular(if is_modular { Some(true) } else { None })
                .decoding_speed(args.decoding_speed)?;
            Ok(())
        })
        .unwrap();

    let mut transcoding_ok = false;
    let mut begin_encode = Instant::now();
    if do_transcode {
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
        basic_info.uses_original_profile = is_lossless as i32;
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
    let pixels = width as u64 * height as u64;
    let throughput_mp = pixels as f64 / (duration_encode.as_secs_f64() * 1_000_000.);
    println!(
        "Encoding and output took {:.2} ms ({throughput_mp:.3} MP/s)",
        duration_encode.as_secs_f64() * 1000.
    );
}
