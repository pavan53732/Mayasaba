// Mayasaba Control Room — composer view-model implementation.
#include "pch.h"

#include "ComposerViewModel.h"

#include <cstdio>

namespace mayasaba::app {

std::string FormatByteSize(std::uint64_t bytes) {
    static const char* kUnits[] = {"B", "KB", "MB", "GB", "TB"};
    double value = static_cast<double>(bytes);
    int unit = 0;
    while (value >= 1024.0 && unit < 4) {
        value /= 1024.0;
        ++unit;
    }
    char buffer[64]{};
    if (unit == 0) {
        std::snprintf(buffer, sizeof(buffer), "%llu %s",
                      static_cast<unsigned long long>(bytes), kUnits[unit]);
    } else {
        std::snprintf(buffer, sizeof(buffer), "%.1f %s", value, kUnits[unit]);
    }
    return std::string(buffer);
}

std::vector<mayasaba::control::AttachmentRef> ComposerViewModel::ToAttachmentRefs() const {
    std::vector<mayasaba::control::AttachmentRef> refs;
    refs.reserve(attachments_.size());
    for (const auto& attachment : attachments_) {
        mayasaba::control::AttachmentRef ref;
        ref.name = attachment.name;
        ref.path = attachment.path;
        ref.size = attachment.size;
        ref.sha256 = attachment.sha256;
        refs.push_back(std::move(ref));
    }
    return refs;
}

}  // namespace mayasaba::app
