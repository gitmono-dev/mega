import { Attachment } from '@gitmono/types/generated'

/**
 * Converts an Attachment object into a more usable format
 */
export function getFileMetadata(
  attachment: Partial<
    Pick<Attachment, 'name' | 'file_type' | 'url' | 'download_url' | 'origami' | 'principle' | 'stitch' | 'link'>
  >
) {
  const { origami, principle, stitch } = attachment

  let name = attachment.name || 'File'

  if (attachment.origami) {
    name = attachment.name || 'Origami prototype'
  } else if (attachment.principle) {
    name = attachment.name || 'Principle prototype'
  } else if (attachment.stitch) {
    name = attachment.name || 'Stitch prototype'
  }

  let openUrl
  const downloadUrl = attachment.download_url
  const fileType = attachment.file_type

  if (origami) openUrl = downloadUrl?.replace('https', 'origami-public')
  if (principle) openUrl = downloadUrl?.replace('https', 'principle')
  if (stitch) openUrl = downloadUrl?.replace('https', 'stitch')

  const qrCode = origami || principle || stitch

  return { name, fileType, downloadUrl, openUrl, qrCode, origami, principle, stitch }
}
