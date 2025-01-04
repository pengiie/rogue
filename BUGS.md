# Bugs that we can deal with later.

## Swapchain resizing
When resizing the swapchain, frame image resources in the executor are always 
remade for some reason. I assume this means other resources may not be caches properly in the
future. Right now we only free up resources on the current cpu frame index, 
so i can see how us skipping a frame could affect that, thats why it may be better 
to retire resources based off the gpu timeline semaphore since that is the source of truth on
what the gpu is working on, and we also simulate the timeline semaphore even for skipped frames.

## Too many gpu frame images and pipelines
Realistically we only need one frame image and pipeline objects since the cpu doesn't perform operations on them. 
Right now for a given pipeline/shader in the frame graph, we create a pipeline for each frame in flight,
and perform caching like for the buffers on them. We do the same thing for images. We can remove the cache timeline for
images and always retire them to the cache at the end of a frame, we could even reuse the same image within the frame but
that would require dependency aliasing. For pipelines we just can have a set of compiled pipelines relating to shaders in a
hashmap. That would fix the pipeline issue of this as well.
