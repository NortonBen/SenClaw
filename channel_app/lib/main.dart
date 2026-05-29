import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'screens/main_shell.dart';
import 'screens/welcome_screen.dart';
import 'services/language_service.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await LanguageService().init();
  runApp(const SenclawApp());
}

class SenclawApp extends StatelessWidget {
  const SenclawApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: LanguageService(),
      builder: (context, _) {
        return MaterialApp(
          title: 'Senclaw Connect',
          debugShowCheckedModeBanner: false,
          locale: LanguageService().currentLocale,
          theme: ThemeData(
            brightness: Brightness.dark,
            primarySwatch: Colors.purple,
            fontFamily: 'Inter',
            useMaterial3: true,
          ),
          home: const Initializer(),
        );
      },
    );
  }
}

class Initializer extends StatefulWidget {
  const Initializer({super.key});

  @override
  State<Initializer> createState() => _InitializerState();
}

class _InitializerState extends State<Initializer> {
  final storage = const FlutterSecureStorage();
  bool? _isPaired;

  @override
  void initState() {
    super.initState();
    _checkPairing();
  }

  Future<void> _checkPairing() async {
    final cid = await storage.read(key: 'channel_id');
    setState(() {
      _isPaired = cid != null;
    });
  }

  @override
  Widget build(BuildContext context) {
    if (_isPaired == null) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }
    return _isPaired! ? const MainShell() : const WelcomeScreen();
  }
}
