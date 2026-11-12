package org.zohara.zoai;

import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.os.Parcelable;
import android.provider.OpenableColumns;
import android.database.Cursor;
import android.util.Base64;

import com.getcapacitor.JSArray;
import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.List;

/**
 * Receives ACTION_SEND / ACTION_SEND_MULTIPLE (text, links, images, other files) from other apps.
 * The page asks for it with ZoShare.consume() at start-up and also listens for 'shareReceived'
 * while the app is already running. Files are handed over as base64 (page code turns them into File objects).
 */
@CapacitorPlugin(name = "ZoShare")
public class ZoShare extends Plugin {
    private static final long MAX_FILE = 8L * 1024 * 1024;
    private static final long MAX_TOTAL = 16L * 1024 * 1024;
    private static final int MAX_FILES = 5;

    private JSObject pending;

    @Override
    public void load() {
        handle(getActivity().getIntent());
    }

    @Override
    protected void handleOnNewIntent(Intent intent) {
        super.handleOnNewIntent(intent);
        if (handle(intent) && pending != null) notifyListeners("shareReceived", pending, true);
    }

    @PluginMethod
    public void consume(PluginCall call) {
        JSObject r = pending != null ? pending : new JSObject();
        pending = null;
        call.resolve(r);
    }

    /** Whether google-services.json was in the build (Firebase configured): Google sign-in and push need it. */
    @PluginMethod
    public void info(PluginCall call) {
        JSObject r = new JSObject();
        int id = getContext().getResources().getIdentifier("google_app_id", "string", getContext().getPackageName());
        r.put("firebase", id != 0);
        call.resolve(r);
    }

    private boolean handle(Intent intent) {
        if (intent == null) return false;
        String action = intent.getAction();
        if (!Intent.ACTION_SEND.equals(action) && !Intent.ACTION_SEND_MULTIPLE.equals(action)) return false;
        JSObject out = new JSObject();
        String text = intent.getStringExtra(Intent.EXTRA_TEXT);
        String subject = intent.getStringExtra(Intent.EXTRA_SUBJECT);
        if (text != null) out.put("text", text);
        if (subject != null) out.put("subject", subject);
        List<Uri> uris = new ArrayList<>();
        if (Intent.ACTION_SEND.equals(action)) {
            Uri u = intent.getParcelableExtra(Intent.EXTRA_STREAM);
            if (u != null) uris.add(u);
        } else {
            ArrayList<Uri> l = intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
            if (l != null) uris.addAll(l);
        }
        ClipData clip = intent.getClipData();
        if (uris.isEmpty() && clip != null) for (int i = 0; i < clip.getItemCount(); i++) if (clip.getItemAt(i).getUri() != null) uris.add(clip.getItemAt(i).getUri());
        JSArray files = new JSArray();
        JSArray skipped = new JSArray();
        long total = 0;
        for (Uri u : uris) {
            if (files.length() >= MAX_FILES) { skipped.put(nameOf(u)); continue; }
            try {
                String mime = getContext().getContentResolver().getType(u);
                String name = nameOf(u);
                ByteArrayOutputStream bo = new ByteArrayOutputStream();
                try (InputStream in = getContext().getContentResolver().openInputStream(u)) {
                    if (in == null) { skipped.put(name); continue; }
                    byte[] buf = new byte[16384];
                    int n;
                    boolean tooBig = false;
                    while ((n = in.read(buf)) > 0) {
                        bo.write(buf, 0, n);
                        if (bo.size() > MAX_FILE || total + bo.size() > MAX_TOTAL) { tooBig = true; break; }
                    }
                    if (tooBig) { skipped.put(name); continue; }
                }
                total += bo.size();
                JSObject f = new JSObject();
                f.put("name", name);
                f.put("mime", mime != null ? mime : "application/octet-stream");
                f.put("data_b64", Base64.encodeToString(bo.toByteArray(), Base64.NO_WRAP));
                files.put(f);
            } catch (Exception e) {
                skipped.put(nameOf(u));
            }
        }
        out.put("files", files);
        if (skipped.length() > 0) out.put("skipped", skipped);
        pending = out;
        // The intent is consumed: a later rotation or resume must not re-deliver it.
        intent.setAction(Intent.ACTION_MAIN);
        return true;
    }

    private String nameOf(Uri u) {
        String name = null;
        try (Cursor c = getContext().getContentResolver().query(u, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) name = c.getString(0);
        } catch (Exception ignored) {}
        if (name == null) name = u.getLastPathSegment() != null ? u.getLastPathSegment() : "shared-file";
        return name;
    }
}
