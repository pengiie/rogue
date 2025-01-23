# Bugs/optimizations that we can deal with later.

## Swapchain destruction on wayland
We get a segfault when destroying the final swapchain when closing the window on wayland. 
This may be hyprland specific since it doesn't happen through xwayland but it doesn't matter
since the app is closing anyways.

## Resizing and fullscreen breaks descriptor sets
Not sure why but when I resize the swapchain a lot, the descriptor set handles become invalid.
I may be destroying descriptor sets when I shouldn't be, however I'm not getting any runtime 
errors, only vk validation layers saying the handle is invalid. This is probably
due to the descriptor set garbage collector not cleaning up references to the descriptor set,
so if it is referenced later through the descriptor layout group, we get an invalid handle.

## Staging buffer is locked when used on gpu.
Right now we lock any staging buffer that is used on the gpu even though the only memory we need
to lock of it is just up to the write pointer. We can make it so we can still submit writes with that
buffer, but to ping pong between different sections within the buffer to optimize the space used.
