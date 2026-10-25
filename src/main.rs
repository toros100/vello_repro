use vello::{kurbo, peniko, wgpu};

// texture size does not seem determinative
const HEIGHT: u32 = 256;
const WIDTH: u32 = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let ad = pollster::block_on(inst.request_adapter(&Default::default()))?;
    println!("{:?}", ad.get_info());
    let (device, queue) = pollster::block_on(ad.request_device(&wgpu::DeviceDescriptor {
        required_limits: ad.limits(),
        ..Default::default()
    }))?;

    // breaks for other values of x1, x2 too
    // e.g. x1 = 100, x2 = 227
    let x1 = 100;
    let x2 = 233;

    // have not found any y != 0 that breaks
    let y = 0;

    let report_cpu = render_line(x1, x2, y, &device, &queue, true)?;
    let report_gpu = render_line(x1, x2, y, &device, &queue, false)?;

    let img_gpu =
        image::RgbaImage::from_raw(WIDTH, HEIGHT, report_gpu.image_data).expect("should fit");
    img_gpu.save("out_gpu.png")?;

    let img_cpu =
        image::RgbaImage::from_raw(WIDTH, HEIGHT, report_cpu.image_data).expect("should fit");
    img_cpu.save("out_cpu.png")?;

    if report_cpu.bad_pixels > 0 {
        panic!("oops, the cpu image should be exact (probably my fault)")
    }

    if report_gpu.bad_pixels > 0 {
        Err(format!("bad pixels: {}", report_gpu.bad_pixels).into())
    } else {
        Ok(())
    }
}

struct Report {
    image_data: Vec<u8>,
    bad_pixels: usize,
}

fn render_line(
    x1: u32,
    x2: u32,
    y: u32,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    use_cpu: bool,
) -> Result<Report, Box<dyn std::error::Error>> {
    let line = kurbo::Line::new(
        (x1 as f64 + 0.5, y as f64 + 0.5),
        (x2 as f64 + 0.5, y as f64 + 0.5),
    );

    let mut scene = vello::Scene::new();

    scene.stroke(
        &kurbo::Stroke::new(1.).with_caps(kurbo::Cap::Square),
        kurbo::Affine::IDENTITY,
        peniko::Color::from_rgb8(0, 0, 0),
        None,
        &line,
    );

    let mut r = vello::Renderer::new(
        device,
        vello::RendererOptions {
            use_cpu,
            ..Default::default()
        },
    )?;

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    r.render_to_texture(
        device,
        queue,
        &scene,
        &tex.create_view(&Default::default()),
        &vello::RenderParams {
            base_color: peniko::Color::WHITE,
            width: WIDTH,
            height: HEIGHT,
            antialiasing_method: vello::AaConfig::Area, // breaks with other AaConfig too
        },
    )?;

    let image_data = read_rgba(device, queue, &tex)?;

    let mut bad_pixels = 0;

    let on_line = |px: u32, py: u32| py == y && x1 <= px && px <= x2;

    for py in 0..HEIGHT {
        for px in 0..WIDTH {
            let off = (py * WIDTH + px) as usize;
            let d: [u8; 4] = image_data[off * 4..off * 4 + 4].try_into().unwrap();
            if on_line(px, py) {
                if d != [0, 0, 0, 255] {
                    bad_pixels += 1;
                }
            } else {
                if d != [255, 255, 255, 255] {
                    bad_pixels += 1;
                }
            }
        }
    }

    Ok(Report {
        bad_pixels,
        image_data,
    })
}

fn read_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    t: &wgpu::Texture,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (w, h) = (t.width(), t.height());
    let row = w * 4;

    let row_padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row_padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        t.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_padded),
                rows_per_image: Some(h),
            },
        },
        t.size(),
    );

    queue.submit([enc.finish()]);

    let s = buf.slice(..);

    s.map_async(wgpu::MapMode::Read, |_| {});

    device.poll(wgpu::PollType::wait_indefinitely())?;

    let view = s.get_mapped_range()?;

    let mut data = vec![];
    for r in view.chunks_exact(row_padded as usize) {
        data.extend_from_slice(&r[..row as usize]);
    }

    Ok(data)
}
