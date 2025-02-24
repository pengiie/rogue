implementing lib;

namespace consts {

public static const float METERS_PER_VOXEL = CONST_METERS_PER_VOXEL;

public static const uint TERRAIN_CHUNK_VOXEL_LENGTH = CONST_TERRAIN_CHUNK_VOXEL_LENGTH;
public static const float TERRAIN_CHUNK_METER_LENGTH = METERS_PER_VOXEL * (float) TERRAIN_CHUNK_VOXEL_LENGTH;

public static const uint VOXEL_MODEL_SCHEMA_ESVO = CONST_VOXEL_MODEL_SCHEMA_ESVO;
public static const uint VOXEL_MODEL_SCHEMA_THC = 3;

public static const uint VOXEL_ATTACHMENT_MAX_ID = 2;
public static const uint VOXEL_ATTACHMENT_COUNT = VOXEL_ATTACHMENT_MAX_ID + 1;

} // namespace consts
