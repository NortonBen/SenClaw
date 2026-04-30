import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'dart:convert';
import 'package:http/http.dart' as http;
import 'chat_screen.dart';
import '../services/language_service.dart';
import '../services/config_service.dart';

class PairingScreen extends StatefulWidget {
  const PairingScreen({super.key});

  @override
  State<PairingScreen> createState() => _PairingScreenState();
}

class _PairingScreenState extends State<PairingScreen> with SingleTickerProviderStateMixin {
  final _config = ConfigService();
  final MobileScannerController controller = MobileScannerController();
  late AnimationController _animationController;
  bool _isProcessing = false;

  @override
  void initState() {
    super.initState();
    _animationController = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 2),
    )..repeat(reverse: true);
  }

  @override
  void dispose() {
    _animationController.dispose();
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    double scanWindowSize = 250;
    
    return Scaffold(
      backgroundColor: Colors.black,
      body: Stack(
        children: [
          // Background Scanner
          MobileScanner(
            controller: controller,
            scanWindow: Rect.fromCenter(
              center: Offset(
                MediaQuery.of(context).size.width / 2,
                MediaQuery.of(context).size.height / 2,
              ),
              width: scanWindowSize,
              height: scanWindowSize,
            ),
            onDetect: (capture) async {
              if (_isProcessing) return;
              final List<Barcode> barcodes = capture.barcodes;
              for (final barcode in barcodes) {
                final code = barcode.rawValue;
                if (code != null && code.startsWith('semaclaw://connect')) {
                  setState(() => _isProcessing = true);
                  await _handleScan(code);
                  break;
                }
              }
            },
          ),
          
          // Cut-out Overlay & Animation
          AnimatedBuilder(
            animation: _animationController,
            builder: (context, child) {
              return CustomPaint(
                painter: ScannerOverlayPainter(
                  scanWindowSize: scanWindowSize,
                  overlayColor: Colors.black.withOpacity(0.7),
                  scanLinePosition: _animationController.value,
                ),
                child: Container(),
              );
            },
          ),

          // Content Layer
          SafeArea(
            child: Column(
              children: [
                // Top Action Bar
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      IconButton(
                        icon: Container(
                          padding: const EdgeInsets.all(5),
                          decoration: BoxDecoration(
                            color: Colors.white.withOpacity(0.2),
                            shape: BoxShape.circle,
                          ),
                          child: const Icon(Icons.close, color: Colors.white, size: 20),
                        ),
                        onPressed: () => Navigator.pop(context),
                      ),
                      const SizedBox(width: 48), // Placeholder for balance
                    ],
                  ),
                ),
                
                const SizedBox(height: 40),
                Text(
                  t('scan_hint'),
                  style: TextStyle(
                    color: Colors.white,
                    fontSize: 18,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 0.5,
                  ),
                ),
                
                const Spacer(),
                
                // Bottom Instructions List - Centered under QR
                Padding(
                  padding: const EdgeInsets.only(bottom: 60),
                  child: Column(
                    children: [
                      Container(
                        constraints: const BoxConstraints(maxWidth: 300),
                        child: Column(
                          children: [
                            _buildInstructionItem(
                              Icons.cloud_download_outlined,
                              t('step_1'),
                            ),
                            const SizedBox(height: 16),
                            _buildInstructionItem(
                              Icons.wifi_lock_outlined,
                              t('step_2'),
                            ),
                            const SizedBox(height: 16),
                            _buildInstructionItem(
                              Icons.account_tree_outlined,
                              t('step_3'),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),

          if (_isProcessing)
            Container(
              color: Colors.black54,
              child: const Center(child: CircularProgressIndicator(color: Colors.white)),
            ),
        ],
      ),
    );
  }

  Widget _buildInstructionItem(IconData icon, String text) {
    return Row(
      children: [
        Icon(icon, color: Colors.purpleAccent.withOpacity(0.8), size: 22),
        const SizedBox(width: 15),
        Expanded(
          child: Text(
            text,
            style: const TextStyle(color: Colors.white70, fontSize: 14),
          ),
        ),
      ],
    );
  }

  Future<void> _handleScan(String code) async {
    try {
      final uri = Uri.parse(code);
      final qrHub = uri.queryParameters['hub'];
      final cid = uri.queryParameters['cid'];
      final key = uri.queryParameters['key'];
      final token = uri.queryParameters['token'];

      debugPrint('--- QR Scan Data ---');
      debugPrint('Hub: $qrHub');
      debugPrint('CID: $cid');
      debugPrint('Key String: $key');
      if (key != null) {
        final decoded = base64.decode(key);
        debugPrint('Decoded Key Length: ${decoded.length} bytes');
      }
      debugPrint('Token: $token');
      debugPrint('-------------------');

      // Read configured hub from storage
      final savedHub = await _config.hubUrl;
      final hub = savedHub ?? qrHub;

      if (hub != null && cid != null && key != null && token != null) {
        try {
          final response = await http.post(
            Uri.parse('${hub.endsWith('/') ? hub.substring(0, hub.length - 1) : hub}/v1/channels/verify'),
            headers: {'Content-Type': 'application/json'},
            body: jsonEncode({
              'channel_id': cid,
              'access_token': token,
            }),
          );

          if (response.statusCode == 200) {
            final data = jsonDecode(response.body);
            if (data['valid'] == true) {
              final grpcUrl = data['grpc_url'] ?? '127.0.0.1:50051';
              
              await _config.savePairingData(
                hubUrl: hub,
                grpcUrl: grpcUrl,
                channelId: cid,
                encryptionKey: key,
                accessToken: token,
              );

              if (!mounted) return;
              Navigator.pushAndRemoveUntil(
                context,
                MaterialPageRoute(builder: (context) => const ChatScreen()),
                (route) => false,
              );
              return;
            } else {
              if (mounted) {
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(content: Text('Verification failed: Invalid token')),
                );
              }
            }
          } else {
            if (mounted) {
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(content: Text('Failed to verify channel with Hub')),
              );
            }
          }
        } catch (e) {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text('Connection error: $e')),
            );
          }
        }
        
        setState(() => _isProcessing = false);
      } else {
        setState(() => _isProcessing = false);
      }
    } catch (e) {
      setState(() => _isProcessing = false);
    }
  }
}

class ScannerOverlayPainter extends CustomPainter {
  final double scanWindowSize;
  final Color overlayColor;
  final double scanLinePosition;

  ScannerOverlayPainter({
    required this.scanWindowSize,
    required this.overlayColor,
    required this.scanLinePosition,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final backgroundPath = Path()..addRect(Rect.fromLTWH(0, 0, size.width, size.height));
    
    final cutoutRect = Rect.fromCenter(
      center: Offset(size.width / 2, size.height / 2),
      width: scanWindowSize,
      height: scanWindowSize,
    );

    final cutoutPath = Path()
      ..addRRect(
        RRect.fromRectAndRadius(
          cutoutRect,
          const Radius.circular(24),
        ),
      );

    final backgroundPaint = Paint()
      ..color = overlayColor
      ..style = PaintingStyle.fill;

    // 1. Draw Background with hole
    final finalPath = Path.combine(
      PathOperation.difference,
      backgroundPath,
      cutoutPath,
    );
    canvas.drawPath(finalPath, backgroundPaint);

    // 2. Draw Corners
    final borderPaint = Paint()
      ..color = Colors.cyanAccent
      ..style = PaintingStyle.stroke
      ..strokeWidth = 5
      ..strokeCap = StrokeCap.round;

    double cornerSize = 30;
    double radius = 24;

    // Top Left
    canvas.drawArc(
      Rect.fromLTWH(cutoutRect.left, cutoutRect.top, radius * 2, radius * 2),
      3.14,
      1.57,
      false,
      borderPaint,
    );
    canvas.drawLine(Offset(cutoutRect.left + radius, cutoutRect.top), Offset(cutoutRect.left + cornerSize, cutoutRect.top), borderPaint);
    canvas.drawLine(Offset(cutoutRect.left, cutoutRect.top + radius), Offset(cutoutRect.left, cutoutRect.top + cornerSize), borderPaint);

    // Top Right
    canvas.drawArc(
      Rect.fromLTWH(cutoutRect.right - radius * 2, cutoutRect.top, radius * 2, radius * 2),
      -1.57,
      1.57,
      false,
      borderPaint,
    );
    canvas.drawLine(Offset(cutoutRect.right - cornerSize, cutoutRect.top), Offset(cutoutRect.right - radius, cutoutRect.top), borderPaint);
    canvas.drawLine(Offset(cutoutRect.right, cutoutRect.top + radius), Offset(cutoutRect.right, cutoutRect.top + cornerSize), borderPaint);

    // Bottom Left
    canvas.drawArc(
      Rect.fromLTWH(cutoutRect.left, cutoutRect.bottom - radius * 2, radius * 2, radius * 2),
      1.57,
      1.57,
      false,
      borderPaint,
    );
    canvas.drawLine(Offset(cutoutRect.left + radius, cutoutRect.bottom), Offset(cutoutRect.left + cornerSize, cutoutRect.bottom), borderPaint);
    canvas.drawLine(Offset(cutoutRect.left, cutoutRect.bottom - cornerSize), Offset(cutoutRect.left, cutoutRect.bottom - radius), borderPaint);

    // Bottom Right
    canvas.drawArc(
      Rect.fromLTWH(cutoutRect.right - radius * 2, cutoutRect.bottom - radius * 2, radius * 2, radius * 2),
      0,
      1.57,
      false,
      borderPaint,
    );
    canvas.drawLine(Offset(cutoutRect.right - cornerSize, cutoutRect.bottom), Offset(cutoutRect.right - radius, cutoutRect.bottom), borderPaint);
    canvas.drawLine(Offset(cutoutRect.right, cutoutRect.bottom - radius), Offset(cutoutRect.right, cutoutRect.bottom - cornerSize), borderPaint);

    // 3. Draw Scanning Line
    final linePaint = Paint()
      ..shader = LinearGradient(
        colors: [
          Colors.blueAccent.withOpacity(0),
          Colors.blueAccent.withOpacity(0.5),
          Colors.blueAccent.withOpacity(0),
        ],
      ).createShader(Rect.fromLTWH(cutoutRect.left, cutoutRect.top + scanWindowSize * scanLinePosition - 10, scanWindowSize, 20))
      ..style = PaintingStyle.fill;

    canvas.drawRect(
      Rect.fromLTWH(
        cutoutRect.left + 5,
        cutoutRect.top + scanWindowSize * scanLinePosition,
        scanWindowSize - 10,
        2,
      ),
      Paint()..color = Colors.cyanAccent.withOpacity(0.8),
    );

    canvas.drawRect(
      Rect.fromLTWH(
        cutoutRect.left + 5,
        cutoutRect.top + scanWindowSize * scanLinePosition - 5,
        scanWindowSize - 10,
        10,
      ),
      linePaint,
    );
  }

  @override
  bool shouldRepaint(covariant ScannerOverlayPainter oldDelegate) {
    return oldDelegate.scanWindowSize != scanWindowSize || 
           oldDelegate.overlayColor != overlayColor ||
           oldDelegate.scanLinePosition != scanLinePosition;
  }
}
