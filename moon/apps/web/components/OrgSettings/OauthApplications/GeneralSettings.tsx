import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import toast from 'react-hot-toast'
import { z } from 'zod'

import { OauthApplication } from '@gitmono/types/generated'
import { Button } from '@gitmono/ui/Button'
import { FormError } from '@gitmono/ui/FormError'
import { UIText } from '@gitmono/ui/Text'
import { TextField } from '@gitmono/ui/TextField'

import { AvatarUploader } from '@/components/AvatarUploader'
import * as SettingsSection from '@/components/SettingsSection'
import { useUpdateOauthApplication } from '@/hooks/useUpdateOauthApplication'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { TransformedFile } from '@/utils/types'

const generalSettingsSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  avatar_path: z.string().optional()
})

function useGeneralSettingsForm(initialValues?: OauthApplication) {
  return useForm({
    defaultValues: {
      name: initialValues?.name || '',
      avatar_path: initialValues?.avatar_path || ''
    },
    resolver: zodResolver(generalSettingsSchema)
  })
}

export function GeneralSettings({ oauthApplication }: { oauthApplication: OauthApplication }) {
  const {
    handleSubmit,
    setValue,
    watch,
    formState: { errors, submitCount }
  } = useGeneralSettingsForm(oauthApplication)
  const { mutate: updateOauthApplication, isPending: isUpdating } = useUpdateOauthApplication({
    id: oauthApplication?.id
  })

  const [avatar, setAvatar] = useState<TransformedFile | null>(null)
  const [avatarError, setAvatarError] = useState<Error | null>(null)
  const isUploadingAvatar = !!avatar && !avatar?.key

  const onSubmit = handleSubmit(async (data) => {
    updateOauthApplication(data, {
      onError: apiErrorToast,
      onSuccess: () => {
        toast('Integration updated')
      }
    })
  })

  const name = watch('name')
  const showErrors = submitCount > 0

  return (
    <form onSubmit={onSubmit}>
      <SettingsSection.Section>
        <SettingsSection.Header>
          <SettingsSection.Title>General settings</SettingsSection.Title>
        </SettingsSection.Header>
        <SettingsSection.Separator />
        <SettingsSection.Body className='space-y-4'>
          <TextField
            label='Name'
            name='name'
            placeholder='Integration name'
            value={name}
            onChange={(value) => setValue('name', value, { shouldValidate: true })}
            inlineError={showErrors && errors.name ? errors.name.message : undefined}
          />
          <div>
            <UIText element='label' secondary weight='font-medium' className='mb-1.5 block' size='text-xs'>
              Avatar
            </UIText>
            <div className='flex'>
              <AvatarUploader
                size='sm'
                onFileUploadError={(_, error) => setAvatarError(error)}
                onFileUploadSuccess={(file, key) => {
                  if (key) {
                    setAvatar({ ...file, key })
                    setValue('avatar_path', key)
                    setAvatarError(null)
                  }
                }}
                src={oauthApplication?.avatar_path ? oauthApplication.avatar_url : undefined}
                resource='OauthApplication'
                onFileUploadStart={(file) => {
                  setAvatar(file)
                  setAvatarError(null)
                }}
              />
            </div>
          </div>
          {avatarError && <FormError>{avatarError.message}</FormError>}
        </SettingsSection.Body>
        <SettingsSection.Footer>
          <Button loading={isUpdating} disabled={isUploadingAvatar} type='submit' variant='primary'>
            Save
          </Button>
        </SettingsSection.Footer>
      </SettingsSection.Section>
    </form>
  )
}
