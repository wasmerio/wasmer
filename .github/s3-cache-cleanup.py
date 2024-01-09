#!/usr/bin/env python3

# Deletes all objects in an S3 bucket that are older than a given number of days.
# Used for cleaning up the custom Github Actions cache.

import boto3
import datetime
import os

# Define the S3 bucket name and the number of days to retain objects
days_to_retain = 7

bucket_name = os.environ['AWS_BUCKET_NAME']
access_key = os.environ['AWS_ACCESS_KEY_ID']
secret_key = os.environ['AWS_SECRET_ACCESS_KEY']
endpoint = os.environ['AWS_ENDPOINT']

# Create a connection to the S3 service
s3 = boto3.resource('s3',
  endpoint_url = endpoint,
  aws_access_key_id = access_key,
  aws_secret_access_key = secret_key,
  region_name = 'auto',
)

bucket = s3.Bucket(bucket_name)

# Calculate the retention date.
cutoff_date = (datetime.datetime.now() - datetime.timedelta(days=days_to_retain))
cutoff_date = cutoff_date.replace(tzinfo=datetime.timezone.utc)

print(f'Deleting all objects in bucket {bucket_name} older than {cutoff_date}...')

total_count = 0
deleted_count = 0

for obj in bucket.objects.all():
    total_count += 1
    if obj.last_modified < cutoff_date:
        print(f'Deleting {obj.key}...')
        obj.delete()
        deleted_count += 1

print(f'Complete! Deleted {deleted_count} objects out of a total {total_count}.')
