fn main() {
    let mut encoder = jexcel::JxlEncoder::new().unwrap();

    let mut basic_info = jexcel::BasicInfo::new();
    basic_info.xsize = 1024;
    basic_info.ysize = 1024;
    basic_info.bits_per_sample = 8;
    basic_info.uses_original_profile = 1;
    encoder.set_basic_info(&basic_info).unwrap();

    let color_encoding = jexcel::ColorEncoding::srgb(jexcel::RenderingIntent::Relative);
    encoder.set_color_encoding(&color_encoding).unwrap();

    let _settings = encoder.create_frame_settings_with(|settings| {
        // d0e3
        settings
            .distance(0.0)?
            .effort(jexcel::Effort::try_from(3i64)?);
        Ok(())
    }).unwrap();

    drop(encoder);
}
