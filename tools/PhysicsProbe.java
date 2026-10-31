import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;

public class PhysicsProbe {
    static final Direction[] DIRS = {Direction.DOWN, Direction.UP, Direction.NORTH, Direction.SOUTH, Direction.WEST, Direction.EAST};

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        EmptyBlockGetter level = EmptyBlockGetter.INSTANCE;
        BlockPos pos = BlockPos.ZERO;
        List<String> fluids = new ArrayList<>();
        for (Fluid f : BuiltInRegistries.FLUID) fluids.add(BuiltInRegistries.FLUID.getKey(f).toString());
        int n = Block.BLOCK_STATE_REGISTRY.size();
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0]), StandardCharsets.UTF_8))) {
            out.print("{\"fluids\":[");
            for (int i = 0; i < fluids.size(); i++) out.print((i > 0 ? "," : "") + "\"" + fluids.get(i) + "\"");
            out.print("],\"states\":[");
            for (int id = 0; id < n; id++) {
                BlockState s = Block.BLOCK_STATE_REGISTRY.byId(id);
                VoxelShape col = s.getCollisionShape(level, pos);
                VoxelShape shape = s.getShape(level, pos);
                long flags = 0;
                flags |= bit(s.isAir(), 0);
                flags |= bit(s.liquid(), 1);
                flags |= bit(s.isSolid(), 2);
                flags |= bit(s.canBeReplaced(), 3);
                flags |= bit(s.isSolidRender(), 4);
                flags |= bit(s.canOcclude(), 5);
                flags |= bit(!col.isEmpty(), 6);
                flags |= bit(!shape.isEmpty(), 7);
                flags |= bit(s.isCollisionShapeFullBlock(level, pos), 8);
                flags |= bit(s.ignitedByLava(), 9);
                flags |= bit(s.isRandomlyTicking(), 10);
                flags |= bit(s.hasLargeCollisionShape(), 11);
                flags |= bit(s.propagatesSkylightDown(), 12);
                flags |= bit(s.useShapeForLightOcclusion(), 13);
                flags |= bit(s.hasBlockEntity(), 14);
                flags |= bit(s.requiresCorrectToolForDrops(), 15);
                flags |= bit(s.isSignalSource(), 16);
                flags |= bit(s.hasOffsetFunction(), 17);
                long faces = 0;
                for (int d = 0; d < 6; d++) {
                    Direction dir = DIRS[d];
                    faces |= bit(s.isFaceSturdy(level, pos, dir, SupportType.FULL), d);
                    faces |= bit(s.isFaceSturdy(level, pos, dir, SupportType.CENTER), 6 + d);
                    faces |= bit(s.isFaceSturdy(level, pos, dir, SupportType.RIGID), 12 + d);
                    faces |= bit(Block.isFaceFull(col, dir), 18 + d);
                    faces |= bit(!col.getFaceShape(dir).isEmpty(), 24 + d);
                    faces |= bit(Block.isFaceFull(s.getFaceOcclusionShape(dir), dir), 30 + d);
                }
                FluidState fs = s.getFluidState();
                int fluid = fluids.indexOf(BuiltInRegistries.FLUID.getKey(fs.getType()).toString());
                String box = "null";
                if (!col.isEmpty()) {
                    AABB b = col.bounds();
                    box = "[" + b.minX + "," + b.minY + "," + b.minZ + "," + b.maxX + "," + b.maxY + "," + b.maxZ + "]";
                }
                Block block = s.getBlock();
                out.print((id > 0 ? "," : "") + "[" + flags + "," + faces + "," + fluid + "," + fs.getAmount() + ","
                        + s.getLightEmission() + "," + s.getLightDampening() + "," + s.getDestroySpeed(level, pos) + ","
                        + block.getExplosionResistance() + "," + block.getFriction() + "," + block.getSpeedFactor() + ","
                        + block.getJumpFactor() + "," + box + "]");
            }
            out.print("]}");
        }
        System.out.println("etats: " + n + ", fluides: " + fluids);
    }

    static long bit(boolean b, int i) {
        return b ? (1L << i) : 0L;
    }
}
