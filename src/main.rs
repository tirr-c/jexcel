use std::fs::File;
use std::io::IsTerminal;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::ExecutableCommand;
use eyre::{Context, OptionExt};
use image::ImageDecoder;
use indicatif::{ProgressState, ProgressStyle};
use rayon::prelude::*;
use tracing_indicatif::span_ext::IndicatifSpanExt;

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
    #[arg(short, long)]
    recursive: bool,
    /// Input file name.
    input: PathBuf,
}

#[derive(Debug)]
struct EncodingStats {
    input_format: image::ImageFormat,
    image_dimension: (u32, u32),
    bits_per_sample: u32,
    is_lossless: bool,
    is_transcoded: bool,
    input_size: u64,
    output_size: u64,
    duration_read_image: Duration,
    duration_decode_image: Duration,
    duration_encode: Duration,
    duration_output: Duration,
}

fn init_subscriber(_args: &Args) {
    use tracing_subscriber::prelude::*;

    let mut stderr = std::io::stderr();
    let is_terminal = stderr.is_terminal();
    if is_terminal {
        stderr.execute(crossterm::style::ResetColor).ok();
    }

    let style = ProgressStyle::with_template("{span_child_prefix}{spinner} {wide_msg} {elapsed}")
        .unwrap()
        .with_key(
            "elapsed",
            |state: &ProgressState, writer: &mut dyn std::fmt::Write| {
                let elapsed = state.elapsed();
                let seconds = elapsed.as_secs();
                let subsecs = elapsed.subsec_millis() / 100;
                write!(writer, "{seconds}.{subsecs}s").ok();
            },
        );
    let indicatif_layer = tracing_indicatif::IndicatifLayer::new().with_progress_style(style);
    let fmt_layer =
        tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer());

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(indicatif_layer)
        .init();
}

fn main() {
    let args = Args::parse();
    init_subscriber(&args);

    if args.recursive {
        let span = tracing::info_span!("collect files", input = %args.input.display());
        span.pb_set_message("Collecting input files");

        let files = span.in_scope(|| {
            let glob = globset::GlobSet::builder()
                .add(globset::Glob::new("**/*.{png,jpg,jpeg,webp}").unwrap())
                .build()
                .expect("failed to compile globset");

            walkdir::WalkDir::new(&args.input)
                .into_iter()
                .filter_map(|entry| {
                    entry
                        .inspect_err(|err| {
                            tracing::error!(%err, "Error while traversing directory");
                        })
                        .ok()
                })
                .filter_map(|entry| {
                    let file_type = entry.file_type();
                    if file_type.is_symlink() {
                        let path = entry.path();
                        tracing::debug!("File \"{}\" is a symlink; not following", path.display());
                        return None;
                    }

                    if !entry.file_type().is_file() {
                        return None;
                    }

                    Some(entry.into_path())
                })
                .filter(|path| {
                    let relpath = path
                        .strip_prefix(&args.input)
                        .expect("cannot strip prefix from input path");
                    glob.is_match(relpath)
                })
                .collect::<Vec<_>>()
        });
        drop(span);

        let parent_span = tracing::info_span!("encode files");
        parent_span.pb_set_style(&ProgressStyle::default_bar());
        parent_span.pb_set_length(files.len() as u64);
        let _guard = parent_span.enter();

        files.into_par_iter().for_each(|path| {
            let _guard = parent_span.enter();

            let relpath = path
                .strip_prefix(&args.input)
                .expect("cannot strip prefix from input path");

            let output_path = args
                .output
                .as_ref()
                .map(|path| path.join(relpath).with_extension("jxl"));

            let file = output_path
                .as_ref()
                .map(|output_path| {
                    if let Some(parent) = output_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    File::create_new(output_path)
                })
                .transpose();

            let file = match file {
                Ok(x) => x,
                Err(err) => {
                    let path = output_path.as_ref().unwrap().display();
                    tracing::error!(%err, "Error creating output file \"{path}\"");
                    parent_span.pb_inc(1);
                    return;
                }
            };

            let span = tracing::info_span!(
                "encode single image",
                input = %relpath.display(),
            );
            span.pb_set_message(&format!("Encoding {}", relpath.display()));
            let _guard = span.entered();

            let stats = match encode_single(&path, file, &args) {
                Ok(x) => x,
                Err(err) => {
                    tracing::error!(%err, "Error encoding image \"{}\"", relpath.display());
                    parent_span.pb_inc(1);
                    return;
                }
            };

            let (width, height) = stats.image_dimension;
            let num_pixels = width as u64 * height as u64;
            tracing::info!(
                "{}: {width} x {height}, {} to {} bytes ({:.2} bpp)",
                relpath.display(),
                if stats.is_transcoded {
                    "transcoded"
                } else {
                    "encoded"
                },
                stats.output_size,
                (stats.output_size * 8) as f64 / num_pixels as f64,
            );

            parent_span.pb_inc(1);
        });
    } else {
        let output = args
            .output
            .as_ref()
            .map(|output| std::fs::File::create(output).wrap_err("failed to create output file"))
            .transpose()
            .unwrap();
        let stats = encode_single(&args.input, output, &args).unwrap();

        let (width, height) = stats.image_dimension;
        tracing::info!(
            "Input: {:?}, {} x {}, {} bpc, {} bytes",
            stats.input_format,
            width,
            height,
            stats.bits_per_sample,
            stats.input_size,
        );

        tracing::info!(
            "{} to {} bytes ({})",
            if stats.is_transcoded {
                "Transcoded"
            } else {
                "Encoded"
            },
            stats.output_size,
            if stats.is_lossless {
                "lossless"
            } else {
                "lossy"
            },
        );

        tracing::info!(
            "Reading input took {:.2} ms",
            stats.duration_read_image.as_secs_f64() * 1000.
        );

        if !stats.is_transcoded {
            tracing::info!(
                "Decoding input took {:.2} ms",
                stats.duration_decode_image.as_secs_f64() * 1000.
            );
        }

        let pixels = width as u64 * height as u64;
        let throughput_mp = pixels as f64 / (stats.duration_encode.as_secs_f64() * 1_000_000.);
        tracing::info!(
            "Encoding took {:.2} ms ({throughput_mp:.3} MP/s)",
            stats.duration_encode.as_secs_f64() * 1000.
        );

        if args.output.is_some() {
            tracing::info!(
                "Writing output took {:.2} ms",
                stats.duration_output.as_secs_f64() * 1000.
            );
        }
    }
}

fn encode_single(
    input: impl AsRef<Path>,
    mut output: Option<File>,
    args: &Args,
) -> eyre::Result<EncodingStats> {
    let mut distance = args
        .distance
        .unwrap_or(if args.force_modular { 0. } else { 1. });
    let is_lossless = distance < 0.01;
    let effort = jexcel::Effort::try_from(args.effort).wrap_err("invalid effort settings")?;
    if is_lossless {
        distance = 0.;
    }
    let is_modular = is_lossless || args.force_modular;

    let begin_read_image = Instant::now();
    let input_buffer = std::fs::read(input).wrap_err("failed to read input")?;
    let input_size = input_buffer.len() as u64;
    let duration_read_image = begin_read_image.elapsed();

    let image = image::ImageReader::new(std::io::Cursor::new(&input_buffer))
        .with_guessed_format()
        .wrap_err("cannot guess image format")?;
    let format = image.format();
    let is_jpeg = image.format() == Some(image::ImageFormat::Jpeg);
    let do_transcode = is_jpeg && !args.force_from_pixels;
    let mut image = image.into_decoder().wrap_err("failed to parse image")?;

    let icc = image.icc_profile().wrap_err("failed to decode image")?;
    let (width, height) = image.dimensions();
    let (num_channels, sample_format, has_alpha) = {
        let color_type = image.color_type();
        let has_alpha = color_type.has_alpha();
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
        (num_channels, sample_format, has_alpha)
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

    let mut encoder = jexcel::JxlEncoder::new().ok_or_eyre("failed to create encoder")?;

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
        .wrap_err("failed to create frame settings")?;

    let mut transcoding_ok = false;
    let frame_guard = tracing::info_span!("add frame").entered();
    let mut begin_encode = Instant::now();
    if do_transcode {
        frame_guard.pb_set_message("Adding JPEG frame");

        begin_encode = Instant::now();
        let mut frame = encoder
            .add_frame(settings)
            .wrap_err("failed to add image frame")?;
        let jpeg_result = frame.jpeg(&input_buffer);

        transcoding_ok = jpeg_result.is_ok();
        if let Err(error) = jpeg_result {
            tracing::warn!(%error, "JPEG transcoding failed, falling back to encoding pixels");
        }
    }

    let mut duration_decode_image = Duration::default();
    if !transcoding_ok {
        frame_guard.pb_set_message("Adding frame");

        let mut basic_info = jexcel::BasicInfo::new();
        basic_info.xsize = width;
        basic_info.ysize = height;
        basic_info.bits_per_sample = bits_per_sample;
        basic_info.uses_original_profile = is_lossless as i32;
        if has_alpha {
            basic_info.num_extra_channels = 1;
            basic_info.alpha_bits = bits_per_sample;
            basic_info.alpha_premultiplied = 0;
        }

        encoder
            .set_basic_info(&basic_info)
            .wrap_err("failed to set basic info")?;

        if let Some(icc) = icc {
            encoder
                .set_icc_profile(&icc)
                .wrap_err("failed to set color encoding")?;
        } else {
            let color_encoding = jexcel::ColorEncoding::srgb(jexcel::RenderingIntent::Relative);
            encoder
                .set_color_encoding(&color_encoding)
                .wrap_err("failed to set color encoding")?;
        }

        let begin_decode_image = Instant::now();
        let mut buffer = vec![0u8; image.total_bytes() as usize];
        image
            .read_image(&mut buffer)
            .wrap_err("failed to decode input image")?;
        duration_decode_image = begin_decode_image.elapsed();

        begin_encode = Instant::now();
        encoder
            .add_frame(settings)
            .wrap_err("failed to add image frame")?
            .color_channels(num_channels, sample_format, &buffer)
            .wrap_err("failed to set image buffer")?;
    }

    encoder.close_input();
    frame_guard.exit();

    let encode_span = tracing::info_span!("encode");
    encode_span.pb_set_message("Encoding frame");

    let (output_size, duration_output) = encode_span.in_scope(|| -> eyre::Result<_> {
        let mut buffer = vec![0u8; 1024 * 1024];
        let mut output_size = 0u64;
        let mut duration_output = Duration::default();

        loop {
            let ret = encoder
                .pull_outputs(&mut buffer)
                .wrap_err("failed to get output data")?;
            output_size += ret.bytes_written() as u64;
            if let Some(output) = &mut output {
                let begin = Instant::now();
                output
                    .write_all(&buffer[..ret.bytes_written()])
                    .wrap_err("failed to write output")?;
                duration_output += begin.elapsed();
            }
            if !ret.need_more_output() {
                break;
            }
        }

        Ok((output_size, duration_output))
    })?;

    let duration_encode_output = begin_encode.elapsed();
    let duration_encode = duration_encode_output - duration_output;

    Ok(EncodingStats {
        input_format: format.unwrap(),
        image_dimension: (width, height),
        bits_per_sample,
        is_lossless,
        is_transcoded: transcoding_ok,
        input_size,
        output_size,
        duration_read_image,
        duration_decode_image,
        duration_encode,
        duration_output,
    })
}
