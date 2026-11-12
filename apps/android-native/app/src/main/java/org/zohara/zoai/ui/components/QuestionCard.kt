package org.zohara.zoai.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.model.QuestionAnswerData
import org.zohara.zoai.model.QuestionData
import org.zohara.zoai.theme.*

@Composable
fun QuestionCard(
    questions: List<QuestionData>,
    onAnswer: (List<QuestionAnswerData>) -> Unit,
    onSkip: () -> Unit,
    modifier: Modifier = Modifier
) {
    if (questions.isEmpty()) return

    var currentStep by remember { mutableStateOf(0) }
    // State per question: selected options Set<String>, and other text
    val answersState = remember {
        mutableStateListOf<MutableState<Pair<Set<String>, String>>>().apply {
            repeat(questions.size) {
                add(mutableStateOf(Pair(emptySet(), "")))
            }
        }
    }
    var isSubmitted by remember { mutableStateOf(false) }
    var summaryText by remember { mutableStateOf("") }
    var isDeclined by remember { mutableStateOf(false) }

    if (isDeclined) {
        Card(
            shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = CardDark),
            modifier = modifier.fillMaxWidth()
        ) {
            Text(
                text = "Declined to answer.",
                style = MaterialTheme.typography.bodyMedium,
                color = DimDark,
                modifier = Modifier.padding(14.dp)
            )
        }
        return
    }

    if (isSubmitted) {
        Card(
            shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = CardDark),
            modifier = modifier.fillMaxWidth()
        ) {
            Text(
                text = "You answered: $summaryText",
                style = MaterialTheme.typography.bodyMedium,
                color = Accent,
                modifier = Modifier.padding(14.dp)
            )
        }
        return
    }

    val q = questions[currentStep]
    val (selectedSet, otherText) = answersState[currentStep].value
    val isMulti = q.multiSelect
    val isFirst = currentStep == 0
    val isLast = currentStep == questions.size - 1

    Card(
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.cardColors(containerColor = CardDark),
        modifier = modifier
            .fillMaxWidth()
            .border(1.dp, Line2Dark, RoundedCornerShape(14.dp))
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            // Header chip & step indicator
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                if (q.header.isNotEmpty()) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(999.dp))
                            .background(ChipBg)
                            .border(1.dp, Line2Dark, RoundedCornerShape(999.dp))
                            .padding(horizontal = 8.dp, vertical = 2.dp)
                    ) {
                        Text(
                            text = q.header,
                            style = MaterialTheme.typography.labelSmall,
                            color = Text2Dark
                        )
                    }
                }
                if (questions.size > 1) {
                    Spacer(modifier = Modifier.weight(1f))
                    Text(
                        text = "Question ${currentStep + 1} of ${questions.size}",
                        style = MaterialTheme.typography.bodySmall,
                        color = DimDark
                    )
                }
            }

            // Question title
            Text(
                text = q.question,
                style = MaterialTheme.typography.titleMedium,
                color = TextDark
            )

            // Options list
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                q.options.forEach { opt ->
                    val isChecked = selectedSet.contains(opt.label)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(9.dp))
                            .background(if (isChecked) Card2Dark else BgDark)
                            .border(
                                1.dp,
                                if (isChecked) Accent else LineDark,
                                RoundedCornerShape(9.dp)
                            )
                            .clickable {
                                val newSet = if (isMulti) {
                                    if (isChecked) selectedSet - opt.label else selectedSet + opt.label
                                } else {
                                    setOf(opt.label)
                                }
                                val newOther = if (!isMulti) "" else otherText
                                answersState[currentStep].value = Pair(newSet, newOther)
                            }
                            .padding(10.dp),
                        verticalAlignment = Alignment.Top
                    ) {
                        if (isMulti) {
                            Checkbox(
                                checked = isChecked,
                                onCheckedChange = null,
                                colors = CheckboxDefaults.colors(
                                    checkedColor = Accent,
                                    checkmarkColor = BgDark
                                )
                            )
                        } else {
                            RadioButton(
                                selected = isChecked,
                                onClick = null,
                                colors = RadioButtonDefaults.colors(
                                    selectedColor = Accent
                                )
                            )
                        }
                        Spacer(modifier = Modifier.width(8.dp))
                        Column {
                            Text(
                                text = opt.label,
                                style = MaterialTheme.typography.bodyMedium,
                                color = TextDark
                            )
                            if (opt.description.isNotEmpty()) {
                                Text(
                                    text = opt.description,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = DimDark
                                )
                            }
                        }
                    }
                }

                // "Other" row
                val otherSelected = otherText.isNotEmpty() || selectedSet.contains("__other__")
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(9.dp))
                        .background(if (otherSelected) Card2Dark else BgDark)
                        .border(
                            1.dp,
                            if (otherSelected) Accent else LineDark,
                            RoundedCornerShape(9.dp)
                        )
                        .padding(10.dp)
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                if (!isMulti) {
                                    answersState[currentStep].value = Pair(setOf("__other__"), otherText)
                                }
                            },
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        if (isMulti) {
                            Checkbox(
                                checked = otherSelected,
                                onCheckedChange = { chk ->
                                    val newSet = if (chk) selectedSet + "__other__" else selectedSet - "__other__"
                                    val newOther = if (chk) otherText else ""
                                    answersState[currentStep].value = Pair(newSet, newOther)
                                },
                                colors = CheckboxDefaults.colors(checkedColor = Accent)
                            )
                        } else {
                            RadioButton(
                                selected = otherSelected,
                                onClick = {
                                    answersState[currentStep].value = Pair(setOf("__other__"), otherText)
                                },
                                colors = RadioButtonDefaults.colors(selectedColor = Accent)
                            )
                        }
                        Spacer(modifier = Modifier.width(8.dp))
                        Text(
                            text = "Other",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextDark
                        )
                    }

                    OutlinedTextField(
                        value = otherText,
                        onValueChange = { newTxt ->
                            val newSet = if (isMulti) {
                                if (newTxt.isNotEmpty()) selectedSet + "__other__" else selectedSet
                            } else {
                                setOf("__other__")
                            }
                            answersState[currentStep].value = Pair(newSet, newTxt)
                        },
                        placeholder = {
                            Text("Type your own answer", color = DimDark, fontSize = 13.sp)
                        },
                        colors = OutlinedTextFieldDefaults.colors(
                            focusedBorderColor = Accent,
                            unfocusedBorderColor = LineDark,
                            focusedTextColor = TextDark,
                            unfocusedTextColor = TextDark
                        ),
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(start = 32.dp, top = 4.dp)
                    )
                }
            }

            // Footer buttons
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                TextButton(onClick = {
                    isDeclined = true
                    onSkip()
                }) {
                    Text("Skip", color = DimDark)
                }

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (!isFirst) {
                        OutlinedButton(
                            onClick = { currentStep-- },
                            colors = ButtonDefaults.outlinedButtonColors(contentColor = TextDark)
                        ) {
                            Text("Back")
                        }
                    }
                    Button(
                        onClick = {
                            if (isLast) {
                                val answers = answersState.map { state ->
                                    val (selected, other) = state.value
                                    val realSelected = selected.filter { it != "__other__" }
                                    QuestionAnswerData(
                                        selected = realSelected,
                                        other = other.trim().ifEmpty { null }
                                    )
                                }
                                summaryText = answers.mapNotNull { a ->
                                    val items = a.selected + listOfNotNull(a.other)
                                    items.joinToString(", ").ifEmpty { null }
                                }.joinToString("; ")
                                isSubmitted = true
                                onAnswer(answers)
                            } else {
                                currentStep++
                            }
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = BgDark)
                    ) {
                        Text(if (isLast) "Send" else "Next")
                    }
                }
            }
        }
    }
}
