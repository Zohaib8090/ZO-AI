import type { CapacitorConfig } from '@capacitor/cli';

// The app is a thin shell around the website: server.url points at the live
// site, so the app updates whenever the site does. Override with ZO_WEB_URL
// (for example a staging server) when syncing.
const url = process.env.ZO_WEB_URL || 'https://zo-ai-server.onrender.com';
const host = new URL(url).host;

const config: CapacitorConfig = {
  appId: 'org.zohara.zoai',
  appName: 'ZO-AI',
  webDir: 'www',
  backgroundColor: '#191919',
  server: {
    url,
    androidScheme: 'https',
    cleartext: false,
    allowNavigation: [
      host,
      '*.firebaseapp.com',
      '*.googleapis.com',
      'accounts.google.com',
      '*.google.com',
      '*.gstatic.com',
      'securetoken.googleapis.com',
      'identitytoolkit.googleapis.com',
    ],
    errorPath: 'offline.html',
  },
  android: { allowMixedContent: false, adjustMarginsForEdgeToEdge: 'force' },
  plugins: {
    SplashScreen: { launchShowDuration: 600, backgroundColor: '#191919', showSpinner: false, androidScaleType: 'CENTER_INSIDE' },
    StatusBar: { style: 'DARK', backgroundColor: '#191919', overlaysWebView: false },
    Keyboard: { resize: 'body' },
    FirebaseAuthentication: { skipNativeAuth: true, providers: ['google.com'] },
    PushNotifications: { presentationOptions: ['badge', 'sound', 'alert'] },
  },
};

export default config;
