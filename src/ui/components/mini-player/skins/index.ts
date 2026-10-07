import type { ComponentType } from "react";
import type { MiniPlayerSkinId } from "../../../settings/miniPlayer";
import { CassetteSkin } from "./CassetteSkin";
import { Y2kSkin } from "./Y2kSkin";
import { VinylSkin } from "./VinylSkin";
import type { MiniSkinProps } from "./shared";

/** Skin components by id. Classic is the original capsule, which owns its own window logic. */
export const SKIN_COMPONENTS: Record<Exclude<MiniPlayerSkinId, "classic">, ComponentType<MiniSkinProps>> = {
  cassette: CassetteSkin,
  vinyl: VinylSkin,
  y2k: Y2kSkin,
};

export type { MiniSkinProps } from "./shared";
