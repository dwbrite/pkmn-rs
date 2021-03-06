use std::{
    sync::Arc,
};

use vulkano::{
    buffer::{BufferUsage, CpuAccessibleBuffer},
    device::{Device, DeviceExtensions, Features},
    instance::{Instance, PhysicalDevice},
    sync::GpuFuture,
    command_buffer::DynamicState,
    device::{Queue},
    pipeline::viewport::Viewport,
    swapchain::{
        self, AcquireError, PresentMode, SurfaceTransform, Swapchain, SwapchainCreationError,
    },
    sync,
    sync::FlushError
};

use crate::window::WindowThing;
use crate::area::Area;
use serde::ser::Serialize;

pub mod window;
pub mod vg;
pub mod area;

#[derive(Default, Debug, Clone)]
pub struct Vertex {
    position: [f32; 2],
}
vulkano::impl_vertex!(Vertex, position);

pub fn get_device_with_queue(physical: PhysicalDevice) -> (Arc<Device>, Arc<Queue>) {
    let queue_family = {
        physical
            .queue_families()
            .find(|&q| q.supports_graphics())
            .expect("couldn't find a graphical queue family")
    };

    let (device, mut queues) = {
        let device_extensions = DeviceExtensions {
            khr_swapchain: true,
            ..DeviceExtensions::none()
        };

        Device::new(
            physical,
            &Features::none(),
            &device_extensions,
            [(queue_family, 0.5)].iter().cloned(),
        )
            .expect("failed to create device")
    };
    let queue = queues.next().unwrap();

    (device, queue)
}

const VIEW_SIZE: [u32; 2] = [240, 160];
const INTERNAL_SIZE: [u32; 2] = [256, 176];

fn main() {
    // Initialize Vulkan(o)
    let extensions = vulkano_win::required_extensions();
    let instance = Instance::new(None, &extensions, None).unwrap();

    let physical = PhysicalDevice::enumerate(&instance).next().unwrap();

    let (device, queue) = get_device_with_queue(physical);



    // Initialize the window + surface
    let window_stuff = WindowThing::init_window(instance.clone(), VIEW_SIZE);
    let surface = window_stuff.surface.clone();



    // Create the swapchain, images
    let (mut swapchain, mut images) = {
        let caps = surface
            .capabilities(physical)
            .expect("failed to get surface capabilities");

        println!("{:?}", caps);

        let alpha = caps.supported_composite_alpha.iter().next().unwrap();
        let format = caps.supported_formats[0].0;

        Swapchain::new(
            device.clone(),
            surface.clone(),
            caps.min_image_count,
            format,
            VIEW_SIZE,
            1,
            caps.supported_usage_flags,
            &queue,
            SurfaceTransform::Identity,
            alpha,
            PresentMode::Fifo,
            true,
            None,
        )
            .expect("failed to create swapchain")
    };



    // Create a viewport based on the swapchain image size
    let mut dynamic_state = DynamicState::none();
    let dimensions = images[0].dimensions();
    let viewport = Viewport {
        origin: [0.0, 0.0],
        dimensions: [dimensions[0] as f32, dimensions[1] as f32],
        depth_range: 0.0..1.0,
    };
    dynamic_state.viewports = Some(vec![viewport]);






    // Done initialization :)))))
    let mut recreate_swapchain = false;

    let mut previous_frame_end = Box::new(sync::now(device.clone())) as Box<dyn GpuFuture>;

    // this is literally meaningless. rename it whenever.
    let mut s_render = s_render::new(queue.clone());
    // why do we get this here?
    let window = surface.window();







    loop {
        previous_frame_end.cleanup_finished();

        // if the window has changed size
        if recreate_swapchain {
            // println!("Recreating swapchain");

            let dimensions = {
                let size = window.read().unwrap().get_size();
                [size.0 as u32, size.1 as u32]
            };

            let (new_swapchain, new_images) = match swapchain.recreate_with_dimension(dimensions) {
                Ok(r) => r,
                Err(SwapchainCreationError::UnsupportedDimensions) => {
                    // println!("Unsupported Dimensions: {:?}", dimensions);
                    continue;
                }
                Err(err) => panic!("{:?}", err),
            };

            swapchain = new_swapchain;
            images = new_images;

            let viewport = Viewport {
                origin: [0.0, 0.0],
                dimensions: [dimensions[0] as f32, dimensions[1] as f32],
                depth_range: 0.0..1.0,
            };
            dynamic_state.viewports = Some(vec![viewport]);

            recreate_swapchain = false;
        }

        let (image_num, acquire_future) =
            match swapchain::acquire_next_image(swapchain.clone(), None) {
                Ok(r) => r,
                Err(AcquireError::OutOfDate) => {
                    // println!("Out of date swapchain");
                    recreate_swapchain = true;
                    continue;
                }
                Err(err) => panic!("{:?}", err),
            };

        let command_buffer = s_render.frame(images[image_num].clone());

        let future = previous_frame_end.join(acquire_future)
            .then_execute(queue.clone(), command_buffer).unwrap()
            .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
            .then_signal_fence_and_flush();

        match future {
            Ok(future) => {
                previous_frame_end = Box::new(future) as Box<_>;
            }
            Err(FlushError::OutOfDate) => {
                recreate_swapchain = true;
                previous_frame_end = Box::new(sync::now(device.clone())) as Box<_>;
            }
            Err(e) => {
                println!("{:?}", e);
                previous_frame_end = Box::new(sync::now(device.clone())) as Box<_>;
            }
        }

        window_stuff.handle_input();

        if window.read().unwrap().should_close() {
            return;
        }
    }
}

mod s_render {
    use std::sync::Arc;
    use vulkano::device::Queue;
    use vulkano::pipeline::{GraphicsPipeline, GraphicsPipelineAbstract};
    use crate::{Vertex, INTERNAL_SIZE, VIEW_SIZE};
    use vulkano::framebuffer::{Subpass, RenderPassAbstract, Framebuffer};
    use vulkano::image::{ImageViewAccess, AttachmentImage, ImageAccess, ImageUsage, ImmutableImage, Dimensions, SwapchainImage};
    use vulkano::format::*;
    use vulkano::command_buffer::{AutoCommandBufferBuilder, AutoCommandBuffer, DynamicState};
    use vulkano::descriptor::descriptor_set::{PersistentDescriptorSet, FixedSizeDescriptorSet, FixedSizeDescriptorSetsPool};
    use vulkano::pipeline::viewport::Viewport;
    use vulkano::buffer::{CpuAccessibleBuffer, BufferUsage};
    use vulkano::sync::GpuFuture;
    use vulkano::command_buffer::pool::standard::StandardCommandPoolAlloc;
    use vulkano::sampler::{Filter, Sampler, UnnormalizedSamplerAddressMode};
    use image::ImageFormat;
    use vulkano::descriptor::{DescriptorSet};
    use vulkano::pipeline::blend::{AttachmentBlend};
    use crate::area::Area;
    use crate::vg::WrappedWindow;

    struct BootyBuffer {
        texture: Arc<dyn ImageViewAccess + Send + Sync>,
        sampler: Arc<Sampler>,
    }

    pub struct RenderThing {
        q: Arc<Queue>,
        render_pass: Arc<dyn RenderPassAbstract + Send + Sync>,
        pipeline: Arc<dyn GraphicsPipelineAbstract + Send + Sync>,
        fbi: Arc<AttachmentImage>,
        vbo: Arc<CpuAccessibleBuffer<[Vertex]>>,
        pool: FixedSizeDescriptorSetsPool<Arc<dyn GraphicsPipelineAbstract + Send + Sync>>,
        ticks: u64,
        bbuf: BootyBuffer,
        camera: (i32, i32),
        forward: bool,
    }

    pub fn new(q: Arc<Queue>) -> RenderThing {
        // Creates a

        let render_pass = {
            Arc::new(
                vulkano::single_pass_renderpass!(
                    q.device().clone(),
                    attachments: {
                        output: {
                            load: Clear,
                            store: Store,
                            format: Format::R8G8B8A8Unorm,
                            samples: 1,
                        }
                    },
                    pass: {
                        color: [output],
                        depth_stencil: {}
                    }
                ).unwrap(),
            )
        };

        let pipeline = {
            let vs = vs::Shader::load(q.device().clone()).unwrap();
            let fs = fs::Shader::load(q.device().clone()).unwrap();

            Arc::new(
                GraphicsPipeline::start()
                    .vertex_input_single_buffer::<Vertex>()
                    .vertex_shader(vs.main_entry_point(), ())
                    .triangle_strip()
                    .viewports_dynamic_scissors_irrelevant(1)
                    .fragment_shader(fs.main_entry_point(), ())
                    .blend_collective(AttachmentBlend::alpha_blending())
                    .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
                    .build(q.device().clone())
                    .unwrap(),
            )
        };

        let img = {
            AttachmentImage::with_usage(
                q.device().clone(),
                INTERNAL_SIZE,
                Format::R8G8B8A8Unorm,
                ImageUsage {
                    transfer_source: true,
                    color_attachment: true,
                    ..ImageUsage::none()
                }
            ).unwrap()
        };

        let vbo = {
            CpuAccessibleBuffer::from_iter(
                q.device().clone(),
                BufferUsage::all(),
                [
                    Vertex { position: [-1.0, -1.0] },
                    Vertex { position: [-1.0, 1.0] },
                    Vertex { position: [1.0, -1.0] },
                    Vertex { position: [1.0, 1.0] },
                ].iter().cloned(),
            ).unwrap()
        };

        let pool : FixedSizeDescriptorSetsPool<Arc<dyn GraphicsPipelineAbstract + Send + Sync>> = FixedSizeDescriptorSetsPool::new(pipeline.clone(), 0);

        let bbuf = {
            let (texture, tex_future) = {
                let image = image::load_from_memory_with_format(include_bytes!("../res/tiles.png"),
                                                                ImageFormat::PNG).unwrap().to_rgba();
                let image_data = image.into_raw().clone();

                ImmutableImage::from_iter(
                    image_data.iter().cloned(),
                    Dimensions::Dim2d { width: 1024, height: 1024 },
                    Format::R8G8B8A8Srgb,
                    q.clone()
                ).unwrap()
            };

            //tex_future.cleanup_finished();
            match tex_future.then_signal_fence_and_flush() {
                Ok(_) => println!("loaded image"),
                Err(_) => println!("shit's fucked")
            }

            let sampler = {
                Sampler::unnormalized(
                    q.device().clone(),
                    Filter::Nearest,
                    UnnormalizedSamplerAddressMode::ClampToEdge,
                    UnnormalizedSamplerAddressMode::ClampToEdge
                ).unwrap()
            };

            BootyBuffer {
                texture,
                sampler
            }
        };

        RenderThing {
            q,
            render_pass,
            pool,
            pipeline,
            fbi: img,
            vbo,
            ticks: 0,
            bbuf,
            camera: (0, 0),
            forward: true
        }
    }

    impl RenderThing {
        pub fn frame(&mut self, _swap_img: Arc<SwapchainImage<WrappedWindow>>) -> AutoCommandBuffer<StandardCommandPoolAlloc>
        {
            let swap_img1 = Box::new(_swap_img.clone()) as Box<ImageAccess + Send + Sync>;
            let swap_img2 = Box::new(_swap_img.clone()) as Box<ImageAccess + Send + Sync>;

            self.ticks += 1;
            let mut offset = ((self.ticks/10)%16) as i32;

            if !self.forward {
                offset = 16 - offset;
            }

            let set = {

                let (w, h) = (20, 20);

                if self.ticks % (16*10) == 0 {
                    let n = if !self.forward { -1 } else { 1 };
                    self.camera.0 += n;
                    self.camera.1 += n;
                }

                if self.camera.0 + 16 >= w {
                    self.forward = false;
                } else if self.camera.0 < 0 {
                    self.forward = true;
                }


                let data_buffer = {
                    let mut _t = Area::from(
                        vec![
                            vec![4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                            vec![1, 1, 2, 1, 1, 4, 4, 1, 2, 3,139, 5, 2, 2, 1, 5, 2, 1, 5, 1],
                            vec![2, 3, 1, 5, 4, 1, 3, 3, 3, 1, 5, 5, 1, 3, 4, 5, 2, 1, 5, 2],
                            vec![3, 3, 3, 2, 4, 1, 3, 3, 2, 2, 2, 3, 5, 4, 2, 3, 2, 2, 1, 4],
                            vec![4, 3, 2, 4, 1, 3, 1, 2, 5, 3, 5, 5, 4, 1, 5, 2, 5, 1, 4, 3],
                            vec![5, 5, 5, 1, 5, 3, 2, 3, 5, 5, 3, 5, 1, 4, 2, 1, 4, 4, 4, 5],
                            vec![2, 2, 5, 3, 3, 4, 5, 5, 2, 3, 2, 4, 1, 2, 2, 3, 1, 2, 3, 2],
                            vec![5, 1, 4, 3, 4, 4, 3, 4, 4, 1, 5, 3, 3, 4, 1, 4, 4, 5, 1, 2],
                            vec![1, 5, 1, 3, 4, 2, 1, 1, 5, 5, 3, 2, 4, 4, 1, 4, 4, 1, 1, 1],
                            vec![2, 3, 5, 2, 2, 2, 2, 4, 1, 2, 5, 5, 4, 1, 5, 3, 2, 2, 3, 4],
                            vec![4, 4, 3, 1, 1, 2, 2, 3, 4, 3, 5, 3, 2, 3, 1, 3, 2, 5, 2, 5],
                            vec![5, 3, 2, 4, 5, 1, 1, 1, 4, 4, 5, 5, 4, 2, 1, 4, 3, 1, 1, 4],
                            vec![4, 2, 4, 3, 2, 2, 4, 3, 3, 5, 3, 1, 5, 1, 4, 5, 4, 5, 3, 2],
                            vec![5, 4, 4, 2, 1, 3, 1, 1, 2, 2, 1, 1, 5, 4, 2, 3, 1, 5, 3, 1],
                            vec![5, 1, 3, 5, 3, 3, 2, 4, 2, 3, 2, 5, 4, 5, 1, 5, 3, 3, 1, 3],
                            vec![3, 2, 4, 5, 5, 4, 5, 5, 5, 2, 3, 5, 2, 4, 5, 2, 4, 3, 2, 5],
                            vec![1, 3, 3, 2, 1, 2, 1, 3, 1, 3, 1, 4, 1, 3, 5, 4, 2, 3, 1, 3],
                            vec![4, 4, 3, 3, 4, 1, 3, 3, 5, 4, 4, 3, 3, 5, 4, 1, 5, 1, 4, 5],
                            vec![1, 4, 2, 1, 3, 1, 4, 2, 2, 2, 2, 5, 4, 3, 4, 1, 3, 2, 1, 2],
                            vec![3, 4, 4, 3, 5, 4, 3, 2, 1, 3, 4, 5, 5, 5, 5, 4, 1, 3, 4, 3]
                        ]
                    );

                    _t.set_tile(2, 1, 0);

                    let mut b = _t.view_slice(self.camera.0 as usize..(self.camera.0+16) as usize, self.camera.1 as usize..(self.camera.1+11) as usize);

                    CpuAccessibleBuffer::from_iter(self.q.device().clone(), BufferUsage::all(),
                                                   b.map.to_owned().into_iter()).expect("failed to create buffer")
                };


                self.pool.next()
                    .add_sampled_image(self.bbuf.texture.clone(), self.bbuf.sampler.clone()).unwrap()
                    .add_buffer(data_buffer.clone()).unwrap()
                    .build().unwrap()
            };

            let framebuffer = Arc::new(
                Framebuffer::start(self.render_pass.clone())
                    .add(self.fbi.clone()).expect("attach fbi failed")
                    .build().unwrap()
            );

            let (dtl, dbr) = {
                let (w1, h1) = (240i32, 160i32);
                let (w2, h2) = (swap_img2.dimensions().width() as i32, swap_img2.dimensions().height() as i32);

                let x_scale = w2 / w1;
                let y_scale = h2 / h1;

                let scale = std::cmp::min(x_scale, y_scale);
                let (ws, hs) = (w1*scale, h1*scale);

                let x_offset = (w2-ws)/2;
                let y_offset = (h2-hs)/2;


                ([x_offset, y_offset, 0], [x_offset+ws, y_offset+hs, 1])
            };

            AutoCommandBufferBuilder::primary_one_time_submit(
                self.q.device().clone(),
                self.q.family(),
            ).unwrap()
                .begin_render_pass(
                    framebuffer.clone(),
                    false,
                    vec![[1.0, 0.0, 1.0, 1.0].into()],
                )
                .unwrap()
                .draw(
                    self.pipeline.clone(),
                    &DynamicState {
                        viewports: Some(vec![Viewport {
                            origin: [0.0, 0.0],
                            dimensions: [INTERNAL_SIZE[0] as f32, INTERNAL_SIZE[1] as f32],
                            depth_range: 0.0..1.0,
                        }]),
                        ..DynamicState::none()
                    },
                    vec![self.vbo.clone()],
                    set,
                    (),
                )
                .unwrap()
                .end_render_pass()
                .unwrap()
                .clear_color_image(swap_img1, ClearValue::Int([0,0,0,1]))
                .unwrap()
                .blit_image(
                    self.fbi.clone(),
                    [offset, offset, 0],
                    [VIEW_SIZE[0] as i32 + offset, VIEW_SIZE[1] as i32 + offset, 1],
                    0, 0,
                    swap_img2,
                    dtl,
                    dbr,
                    0, 0, 1,
                    Filter::Nearest
                )
                .unwrap()
                .build()
                .unwrap()
        }
    }






    mod vs {
        vulkano_shaders::shader! {
        ty: "vertex",
        src: "
#version 450
layout(location = 0) in vec2 position;
void main() {
    gl_Position = vec4(position, 0.0, 1.0);
}"
        }
    }

    mod fs {
        vulkano_shaders::shader! {
        ty: "fragment",
        src: "
#version 450

layout(location = 0) out vec4 f_color;
layout(set = 0, binding = 0) uniform sampler2D tex;


layout(set = 0, binding = 1) buffer Data {
    int grid[11][16];
} data;

vec4 getPixel(in ivec2 pxCoords) {
    ivec2 tSize = ivec2(16);

    ivec2 offset = pxCoords % tSize;
    ivec2 idx = pxCoords / tSize;

    int value = data.grid[idx.y][idx.x];

    int ys = tSize.y * (value/64);
    int xs = tSize.x * (value/64 + value%64);
    ivec2 sc = ivec2(xs, ys);

    return texture(tex, sc + offset);
}

void main() {
    ivec2 pxCoords = ivec2(gl_FragCoord.xy);
    f_color = getPixel(pxCoords);
}"
        }
    }
}

mod server {
    use std::collections::HashMap;
    use crate::area::Area;
    use std::rc::Rc;
    use actix::*;
    use crate::common::Ping;
    use std::time::Duration;

    struct Game {
        areas: HashMap<String, Rc<Area>>,
        live_areas: Vec<String>,
        clients: HashMap<usize, ClientAreas>,
        counter: usize,
        addr: Recipient<Ping>
    }

    impl Actor for Game {
        type Context = Context<Game>;
    }

    impl Handler<Ping> for Game {
        type Result = ();

        fn handle(&mut self, msg: Ping, ctx: &mut Context<Self>) {
            self.counter += 1;
            if self.counter > 10 {
                System::current().stop();
            } else {
                println!("Ping received {:?}", msg.id);

                // wait 100ns
                ctx.run_later(Duration::new(0, 100), move |act, _| {
                    act.addr.do_send(Ping{id: msg.id + 1});
                });
            }
        }
    }

    // TODO: set_areas()
    // TODO: keepalive()
    struct ClientAreas {
        areas: Vec<String>,
        keepalive_tick: u64
    }
}

mod common {
    use actix::*;

    #[derive(Message)]
    pub struct Ping { pub id: usize }
}

mod client {
    use actix::*;

    mod camera {

    }
}


