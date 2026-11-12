/** Resize a photo to JPEG, removing metadata such as EXIF orientation data. */
export async function resizeImage(
  file: File,
  longEdge: number,
  quality: number,
): Promise<Uint8Array> {
  const image = await createImageBitmap(file);
  try {
    const scale = Math.min(1, longEdge / Math.max(image.width, image.height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(image.width * scale));
    canvas.height = Math.max(1, Math.round(image.height * scale));
    const context = canvas.getContext("2d");
    if (!context) throw new Error("This photo could not be resized.");
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob(
        (value) => (value ? resolve(value) : reject(new Error("This photo could not be resized."))),
        "image/jpeg",
        quality,
      );
    });
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    image.close();
  }
}
