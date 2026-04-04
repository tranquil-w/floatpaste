import { convertFileSrc, invoke } from "@tauri-apps/api/core";

type ImageUrlResolverDeps = {
  isTauriRuntime: () => boolean;
  resolveImagePath: (imagePath: string) => Promise<string>;
  convertFileSrc: (absolutePath: string) => string;
};

export const MOCK_IMAGE_URL = `data:image/svg+xml,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150" viewBox="0 0 200 150"><rect width="200" height="150" rx="8" fill="#666"/><text x="100" y="82" text-anchor="middle" fill="#ccc" font-size="14">图片预览</text></svg>',
)}`;

export function createImageUrlResolver({
  isTauriRuntime: isTauriRuntimeFn,
  resolveImagePath: resolveImagePathFn,
  convertFileSrc: convertFileSrcFn,
}: ImageUrlResolverDeps) {
  return async (imagePath: string | null): Promise<string | null> => {
    if (!imagePath) {
      return null;
    }

    if (!isTauriRuntimeFn()) {
      return MOCK_IMAGE_URL;
    }

    try {
      const absolutePath = await resolveImagePathFn(imagePath);
      return convertFileSrcFn(absolutePath);
    } catch {
      return null;
    }
  };
}

export const getImageUrl = createImageUrlResolver({
  isTauriRuntime: () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window,
  resolveImagePath: (imagePath) => invoke("resolve_image_path", { imagePath }),
  convertFileSrc,
});
