# Renderer

The voxel renderer is a global illumination implementation with per-voxel lighting characteristics such as albedo, material, etc.. The renderer is a multi-step process in order to optimize the process and apply temporal effects with previous frame data.

## Irradiance caching

lowkey a work in progress.

## Pipline

### Beam optimization

This pass will partition the full resolution render target into texels of size (nxn). For the corners of each texel, a ray will be shot following the camera's perspective matrix so that the
four corners form a "beam".

Each ray will only traverse until analytically the current intersection would possibly fill less than the current texel. The minimum of these is calculated and in the full resolution primary ray pass, all the rays in the respective texel will start at a t-value determined by the beam optimization. This way voxels can traverse air much quicker since it only has to be done 4/(n*n) times that it would've had for texels of size n.

One thing to note is there is pros and cons to a large or small n size:
 - The greater the n size, the less the rays could possibly traverse since intersected objects would have to be very coarse or else the beam optimization will end early, however less rays would have to do calculations in the full resolution pass.
 - The lesser the n size, the more the rays could traverse due to intersected objects able to be fine grained, however more ray tests would have to be performed where the optimization is equivalent to the full res pass, if not, worse, at n <= 2.

### Primary ray pass

This pass will shoot primary rays starting at the defined value from the beam optimization. These rays will figure out the irradiance cache voxel that is being intersected and enqueue it to a hash set of voxels to run an irradiance sample for.

### Irradiance cache samples

This will use the list of irradiance voxel positions to add to a hashmap and compute an irradiance sample for the hashmap entry.

### Lighting calculation pass

This will use cached results from the primary ray pass of the irradiance cache voxel entry and will fetch that colorizing the pixel.
