# Physics related tasks

## Capsule and box collider detection and resolution.

This will serve a basic test on whether we have implemented physics correctly
before we go on to more difficult collider and physics types.

## Voxel model collider

This will be the collider information for a voxel model, initially
the SFT. This will be more optimized than the model since
only edges will be contained, still yet to decide on the data structure for that.

It needs to be traversed quickly and memory efficient so that would infer a list,
however for the detection we need to easily see if two voxel of two models are intersections,
that is a rotation of one model overlayed onto another to match the same basis. Check similar voxels like this
would need some sort of spatial data structure to make it optimized, probably go with a hashmap for this.
