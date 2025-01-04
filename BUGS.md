# Bugs that we can deal with later.

## Swapchain resizing
When resizing the swapchain, frame image resources in the executor are always 
remade for some reason. I assume this means other resources may not be caches properly in the
future. Right now we only free up resources on the current cpu frame index, 
so i can see how us skipping a frame could affect that, thats why it may be better 
to retire resources based off the gpu timeline semaphore since that is the source of truth on
what the gpu is working on, and we also simulate the timeline semaphore even for skipped frames.
