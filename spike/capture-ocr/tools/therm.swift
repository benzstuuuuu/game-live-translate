// Prints ProcessInfo.thermalState: 0 nominal, 1 fair, 2 serious, 3 critical.
import Foundation
print(ProcessInfo.processInfo.thermalState.rawValue)
