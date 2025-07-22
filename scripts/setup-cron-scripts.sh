#!/bin/bash

REGENERATE_SLL_CERTIFICATES_SCRIPT="/root/cairovm.codes-server/scripts/regenerate-ssl-certificates.sh"
LOG_FILE1="/root/cairovm.codes-server/scripts/logs/regenerate-ssl-certificates-cron-job.log"

# Create log file directory if it does not exist
mkdir -p "$(dirname "$LOG_FILE1")"

if [ ! -f "$REGENERATE_SLL_CERTIFICATES_SCRIPT" ]; then
    echo "Error: script $REGENERATE_SLL_CERTIFICATES_SCRIPT does not exist."
    exit 1
fi

# Delete existing cron jobs
(crontab -l 2>/dev/null | grep -v "$REGENERATE_SLL_CERTIFICATES_SCRIPT") | crontab -

# Add new cron job: every first day of the month at 00:00
(crontab -l 2>/dev/null; echo "0 0 1 * * $REGENERATE_SLL_CERTIFICATES_SCRIPT >> $LOG_FILE1 2>&1") | crontab -

echo "0 0 1 * * $REGENERATE_SLL_CERTIFICATES_SCRIPT >> $LOG_FILE1 2>&1"
echo "Cron jobs updated."