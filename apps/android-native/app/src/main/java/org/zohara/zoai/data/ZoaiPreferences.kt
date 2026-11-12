package org.zohara.zoai.data

import android.content.Context
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

val Context.dataStore by preferencesDataStore(name = "zoai_settings")

class ZoaiPreferences(private val context: Context) {
    companion object {
        val SERVER_URL = stringPreferencesKey("server_url")
        val AUTH_TOKEN = stringPreferencesKey("auth_token")
        val USER_EMAIL = stringPreferencesKey("user_email")
        val SELECTED_MODEL = stringPreferencesKey("selected_model")
        val SELECTED_PROVIDER = stringPreferencesKey("selected_provider")
        val PERMISSION_MODE = stringPreferencesKey("permission_mode")
        val NOTIFY_ANSWERS = booleanPreferencesKey("notify_answers")
        val NOTIFY_NEEDS_YOU = booleanPreferencesKey("notify_needs_you")

        const val DEFAULT_SERVER = "https://zo-ai-server.onrender.com"
    }

    val serverUrl: Flow<String> = context.dataStore.data.map { it[SERVER_URL] ?: DEFAULT_SERVER }
    val authToken: Flow<String?> = context.dataStore.data.map { it[AUTH_TOKEN] }
    val userEmail: Flow<String?> = context.dataStore.data.map { it[USER_EMAIL] }
    val selectedModel: Flow<String> = context.dataStore.data.map { it[SELECTED_MODEL] ?: "claude-3-5-sonnet" }
    val selectedProvider: Flow<String> = context.dataStore.data.map { it[SELECTED_PROVIDER] ?: "anthropic" }
    val permissionMode: Flow<String> = context.dataStore.data.map { it[PERMISSION_MODE] ?: "Auto" }
    val notifyNeedsYou: Flow<Boolean> = context.dataStore.data.map { it[NOTIFY_NEEDS_YOU] ?: true }

    suspend fun setServerUrl(url: String) {
        context.dataStore.edit { it[SERVER_URL] = url }
    }

    suspend fun setAuth(token: String?, email: String?) {
        context.dataStore.edit {
            if (token != null) it[AUTH_TOKEN] = token else it.remove(AUTH_TOKEN)
            if (email != null) it[USER_EMAIL] = email else it.remove(USER_EMAIL)
        }
    }

    suspend fun setModel(model: String, provider: String) {
        context.dataStore.edit {
            it[SELECTED_MODEL] = model
            it[SELECTED_PROVIDER] = provider
        }
    }

    suspend fun setPermissionMode(mode: String) {
        context.dataStore.edit { it[PERMISSION_MODE] = mode }
    }
}
