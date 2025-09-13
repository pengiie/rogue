# Voxel related task

## Voxel world transaction/database system

### Chunks that span more than just a "chunk"

Aka do LODs where we draw a chunk that fills two chunks, but double (more like 2^3 actually) the scale of the model.
We can then do this up the region tree and make the leaves variable, similar to the SFT, LOD can be per region possibly.

This would then save on uploading to the gpu and also generation time since we only need to generate the LOD (Can do async finer
resolution generation later).

We also need to change the terrain information uploaded to the gpu to a sparse tree format that is quick to traverse on the gpu.
In addition, we need to find an appriate lipshitz constant to use for the cascaded noise generation which should
ideally make noise generation much quicker.

Current issues this would fix:
- Render distance can be increased.
- Iterator, noise, and storage can be cascaded, thus faster
- Increased render speed on shadow rays since they often have to traverse out of the chunk tree.

### Per chunk/region concurrent transaction/command system.

This would use a multiple producer, single consumer channel system to recieve commands.
Each command will return an id which can then be used to track the commands status and order of execution (maybe?).
We would ensure gpu uploading is also done async so we don't block the main thread.

Current issues this would fix:
- Gameloop thread won't block when chunk are updated.
- Edits can be tracked and undoed by saving the area being affected by a command.
- Normals calculated only based on volume affected by command.

### Partial gpu model uploads

This would save on the time we spend doing gpu memory transfers.
Specifically the areas that are most important for this are the
SFT and the terrain renderable information (this will also be changed when chunks are cascaded based on LOD).

Can also storage model as compressed format and change to pointer based only
when editing to save on overall memory usage.

Current issues this would fix:
- Time spent uploading to gpu will be reduced.

