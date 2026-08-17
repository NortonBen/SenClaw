import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui' as ui;

/// Longest edge an attached image is downscaled to before upload.
///
/// Matches the size the Anthropic API resizes to internally, and keeps a phone
/// photo under the 5 MB per-image request limit — a 12MP shot base64s to well
/// over that and fails the whole turn, not just the attachment.
const int kMaxImageEdge = 1568;

/// Build the `{mimeType, dataUrl}` attachment map the daemon expects from raw
/// picked bytes, downscaling anything larger than [kMaxImageEdge].
///
/// Re-encodes as PNG when it resizes (`toByteData` offers no JPEG), so the
/// returned `mimeType` can differ from the source. On any decode failure the
/// original bytes are passed through unchanged: a too-large image the model
/// might still accept beats no image at all.
Future<Map<String, String>> buildImageAttachment(
  Uint8List bytes,
  String mimeType,
) async {
  try {
    final decoded = await ui.instantiateImageCodec(bytes);
    final frame = await decoded.getNextFrame();
    final image = frame.image;
    final longEdge =
        image.width > image.height ? image.width : image.height;
    if (longEdge <= kMaxImageEdge) {
      image.dispose();
      return _attachment(bytes, mimeType);
    }

    // Re-decode at the target size — the codec scales during decode, so a huge
    // source never materializes at full resolution.
    final scale = kMaxImageEdge / longEdge;
    final target = await ui.instantiateImageCodec(
      bytes,
      targetWidth: (image.width * scale).round(),
      targetHeight: (image.height * scale).round(),
    );
    image.dispose();
    final scaledFrame = await target.getNextFrame();
    final png =
        await scaledFrame.image.toByteData(format: ui.ImageByteFormat.png);
    scaledFrame.image.dispose();
    if (png == null) return _attachment(bytes, mimeType);
    return _attachment(png.buffer.asUint8List(), 'image/png');
  } catch (_) {
    return _attachment(bytes, mimeType);
  }
}

Map<String, String> _attachment(Uint8List bytes, String mimeType) => {
      'mimeType': mimeType,
      'dataUrl': 'data:$mimeType;base64,${base64Encode(bytes)}',
    };

/// MIME for a picked image file's extension, defaulting to PNG.
String mimeForExtension(String? extension) {
  final ext = (extension ?? 'png').toLowerCase();
  return ext == 'jpg' || ext == 'jpeg' ? 'image/jpeg' : 'image/$ext';
}

/// Extensions the picker treats as images. Everything else is sent as a
/// document: saved server-side, its text extracted into the prompt.
const _imageExtensions = {'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'};

bool isImageExtension(String? extension) =>
    _imageExtensions.contains((extension ?? '').toLowerCase());

/// Largest non-image attachment accepted, matching the daemon's own cap
/// (`MAX_DOC_BYTES` in src/agent/documents.rs).
const int kMaxDocBytes = 32 * 1024 * 1024;

/// MIME for a picked document, guessed from its extension.
///
/// The daemon re-derives the type from the filename anyway, so an unknown
/// extension can safely fall back to a generic binary type.
String documentMimeForExtension(String? extension) {
  switch ((extension ?? '').toLowerCase()) {
    case 'txt':
      return 'text/plain';
    case 'md':
    case 'markdown':
      return 'text/markdown';
    case 'csv':
      return 'text/csv';
    case 'json':
      return 'application/json';
    case 'xml':
      return 'application/xml';
    case 'yaml':
    case 'yml':
      return 'application/yaml';
    case 'html':
    case 'htm':
      return 'text/html';
    case 'pdf':
      return 'application/pdf';
    case 'docx':
      return 'application/vnd.openxmlformats-officedocument.wordprocessingml.document';
    default:
      return 'application/octet-stream';
  }
}

/// Build the attachment map for a non-image file — no resize, no re-encode.
Map<String, String> buildDocumentAttachment(
  Uint8List bytes,
  String mimeType,
  String name,
) =>
    {
      'mimeType': mimeType,
      'name': name,
      'dataUrl': 'data:$mimeType;base64,${base64Encode(bytes)}',
    };
