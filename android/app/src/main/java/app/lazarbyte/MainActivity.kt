package app.lazarbyte

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.lazarbyte_ffi.bridgeVersion

/**
 * Faz 6, step 1: proof that the app loads the Rust bridge and can call it.
 * Everything else -- source selection, scanning, results -- follows in the
 * next steps; this screen is honest about what is not wired up yet.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val version = bridgeVersion()
        setContent {
            MaterialTheme {
                ScaffoldScreen(version)
            }
        }
    }
}

@Composable
private fun ScaffoldScreen(version: String) {
    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Lazarbyte", style = MaterialTheme.typography.headlineMedium)
        Text(
            "bridge version $version",
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(top = 8.dp),
        )
        Text(
            "Scanning is not wired up yet (Faz 6, step 1).",
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(top = 16.dp),
        )
    }
}
