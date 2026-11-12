package org.zohara.zoai;

import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.os.Build;
import android.os.Bundle;

import com.getcapacitor.BridgeActivity;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
        registerPlugin(ZoShare.class);
        super.onCreate(savedInstanceState);
        createAnswersChannel();
    }

    /** Channel 'answers' must exist before the first FCM message arrives, even if the page never ran. */
    private void createAnswersChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        NotificationManager nm = getSystemService(NotificationManager.class);
        if (nm == null) return;
        NotificationChannel ch = new NotificationChannel("answers", getString(R.string.channel_answers), NotificationManager.IMPORTANCE_DEFAULT);
        ch.setDescription(getString(R.string.channel_answers_desc));
        nm.createNotificationChannel(ch);
    }
}
