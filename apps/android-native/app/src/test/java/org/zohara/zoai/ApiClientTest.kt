package org.zohara.zoai

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.zohara.zoai.model.QuestionAnswerData
import org.zohara.zoai.model.QuestionData
import org.zohara.zoai.model.QuestionOptionData

class ApiClientTest {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    @Test
    fun testQuestionDataSerialization() {
        val q = QuestionData(
            question = "Which database?",
            header = "Database",
            multiSelect = false,
            options = listOf(
                QuestionOptionData("PostgreSQL", "SQL database"),
                QuestionOptionData("SQLite", "Embedded database")
            )
        )
        val serialized = json.encodeToString(QuestionData.serializer(), q)
        assertNotNull(serialized)

        val deserialized = json.decodeFromString(QuestionData.serializer(), serialized)
        assertEquals("Which database?", deserialized.question)
        assertEquals("Database", deserialized.header)
        assertEquals(false, deserialized.multiSelect)
        assertEquals(2, deserialized.options.size)
        assertEquals("PostgreSQL", deserialized.options[0].label)
    }

    @Test
    fun testQuestionAnswerSerialization() {
        val answer = QuestionAnswerData(
            selected = listOf("PostgreSQL"),
            other = "version 16"
        )
        val serialized = json.encodeToString(QuestionAnswerData.serializer(), answer)
        val deserialized = json.decodeFromString(QuestionAnswerData.serializer(), serialized)
        assertEquals(listOf("PostgreSQL"), deserialized.selected)
        assertEquals("version 16", deserialized.other)
    }
}
