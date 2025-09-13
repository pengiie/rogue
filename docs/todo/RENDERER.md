# Renderer related task

## Per-voxel lighting calculation

For each ray, we check the voxel that was hit, and append the position that needs to be checked to a list, possibly hashmap.
Then for each fragment, store the voxel address in that list. Then in the next pass, it is computer where we iterate through
the list and update the lighting for the voxel, that can get into more specifics later but the important part is
each voxel has an associated albedo now, this is affected by indirect and direct light ideally. Then in the next pass,
for each pixel we retrieve the voxel that now has the albedo in the spatial hashmap, and write it to the image.

Possibly if we store each coordinate too, we can do some lighting in screen-space using the depth buffer.

Overall the passes are in this order:

1. Initial ray traversal with voxel lighting request.
2. Compute voxel lighting and store albedo in spatial hashmap.
3. Write calculated voxel lighting info to the output image.
