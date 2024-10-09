use std::collections::HashMap;

pub struct PipelineManager {
    pipelines: HashMap<PipelineId, Pipeline>,
    id_counter: u64,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
            id_counter: 0,
        }
    }
}

type PipelineId = u64;

pub struct PipelineHandle {
    id: PipelineId,
}

enum Pipeline {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}
